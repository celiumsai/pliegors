// SPDX-License-Identifier: GPL-3.0-only

use super::wire::{
    CommitOutcome, ERROR_MEDIA_TYPE, MAX_WIRE_BYTES, PRODUCT_MEDIA_TYPE, ProductCapabilities,
    ProductError, ProductLimits, ProductOperation, ProductResponse, TransactionStatus, WireError,
    decode_error, decode_response, encode_request,
};
use bytes::Bytes;
use http::{HeaderMap, Request, StatusCode};
use http_body_util::{BodyExt, Empty, Full, Limited};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use std::fmt;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;

#[derive(Clone)]
pub struct NativeHttpClient {
    endpoint: SocketAddr,
    timeout: Duration,
}

impl NativeHttpClient {
    pub fn new(endpoint: SocketAddr, timeout: Duration) -> Result<Self, TransportError> {
        if !endpoint.ip().is_loopback() {
            return Err(TransportError::NonLoopbackEndpoint(endpoint));
        }
        if timeout.is_zero() {
            return Err(TransportError::Timeout);
        }
        Ok(Self { endpoint, timeout })
    }

    pub async fn capabilities(
        &self,
        request_id: u128,
    ) -> Result<ProductCapabilities, TransportError> {
        let request = Request::builder()
            .method("GET")
            .uri("/v2/capabilities")
            .header("host", self.endpoint.to_string())
            .header(
                "accept",
                format!("{PRODUCT_MEDIA_TYPE}, {ERROR_MEDIA_TYPE}"),
            )
            .header("x-hyphae-request-id", request_id.to_string())
            .body(Empty::<Bytes>::new())
            .map_err(|_| TransportError::Protocol(WireError::Malformed))?;
        match self.send(request, request_id).await? {
            ProductResponse::Capabilities(capabilities) => Ok(capabilities),
            _ => Err(TransportError::UnexpectedResponse),
        }
    }

    pub async fn get(
        &self,
        key: &[u8],
        request_id: u128,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        let response = self
            .execute(ProductOperation::Get(key), None, request_id)
            .await?;
        match response {
            ProductResponse::Value(value) => Ok(value),
            _ => Err(TransportError::UnexpectedResponse),
        }
    }

    pub async fn set(
        &self,
        key: &[u8],
        value: &[u8],
        idempotency_token: u128,
        request_id: u128,
    ) -> Result<(), TransportError> {
        let response = self
            .execute(
                ProductOperation::Set { key, value },
                Some(idempotency_token),
                request_id,
            )
            .await?;
        match response {
            ProductResponse::Commit(CommitOutcome::Committed(receipt))
                if receipt.durability == 0 =>
            {
                Ok(())
            }
            ProductResponse::Commit(CommitOutcome::OutcomeUnknown(transaction_id)) => {
                Err(TransportError::OutcomeUnknown(transaction_id))
            }
            ProductResponse::Commit(_) => Err(TransportError::NonStrictCommit),
            _ => Err(TransportError::UnexpectedResponse),
        }
    }

    pub async fn resolve_idempotency(
        &self,
        idempotency_token: u128,
        request_id: u128,
    ) -> Result<TransactionStatus, TransportError> {
        let response = self
            .execute(
                ProductOperation::TransactionStatusByIdempotency(idempotency_token),
                None,
                request_id,
            )
            .await?;
        match response {
            ProductResponse::TransactionStatus(status) => Ok(status),
            _ => Err(TransportError::UnexpectedResponse),
        }
    }

    async fn execute(
        &self,
        operation: ProductOperation<'_>,
        idempotency_token: Option<u128>,
        request_id: u128,
    ) -> Result<ProductResponse, TransportError> {
        let now = unix_micros()?;
        let deadline = now
            .checked_add(
                i64::try_from(self.timeout.as_micros()).map_err(|_| TransportError::Timeout)?,
            )
            .ok_or(TransportError::Timeout)?;
        let body = encode_request(
            operation,
            now,
            Some(deadline),
            idempotency_token,
            ProductLimits::default(),
        )?;
        let request = Request::builder()
            .method("POST")
            .uri("/v2/execute")
            .header("host", self.endpoint.to_string())
            .header("content-type", PRODUCT_MEDIA_TYPE)
            .header(
                "accept",
                format!("{PRODUCT_MEDIA_TYPE}, {ERROR_MEDIA_TYPE}"),
            )
            .header("x-hyphae-request-id", request_id.to_string())
            .header("x-hyphae-deadline-micros", deadline.to_string())
            .body(Full::new(Bytes::from(body)))
            .map_err(|_| TransportError::Protocol(WireError::Malformed))?;
        self.send(request, request_id).await
    }

    async fn send<B>(
        &self,
        request: Request<B>,
        request_id: u128,
    ) -> Result<ProductResponse, TransportError>
    where
        B: hyper::body::Body<Data = Bytes> + Send + 'static,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        if request_id == 0 {
            return Err(TransportError::RequestId);
        }
        let endpoint = self.endpoint;
        let future = async move {
            let stream = TcpStream::connect(endpoint)
                .await
                .map_err(TransportError::Io)?;
            let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
                .await
                .map_err(TransportError::Http)?;
            let connection = tokio::spawn(connection);
            let response = sender
                .send_request(request)
                .await
                .map_err(TransportError::Http)?;
            let status = response.status();
            validate_request_id(response.headers(), request_id)?;
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| TransportError::MediaType("missing".to_owned()))?
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned();
            let body = Limited::new(response.into_body(), MAX_WIRE_BYTES)
                .collect()
                .await
                .map_err(|_| TransportError::Body)?
                .to_bytes();
            drop(sender);
            connection.abort();
            if status == StatusCode::OK && content_type == PRODUCT_MEDIA_TYPE {
                return decode_response(&body).map_err(Into::into);
            }
            if status != StatusCode::OK && content_type == ERROR_MEDIA_TYPE {
                return Err(decode_error(&body)?.into());
            }
            Err(TransportError::MediaType(content_type))
        };
        tokio::time::timeout(self.timeout, future)
            .await
            .map_err(|_| TransportError::Timeout)?
    }
}

#[derive(Debug)]
pub enum TransportError {
    NonLoopbackEndpoint(SocketAddr),
    Timeout,
    Io(std::io::Error),
    Http(hyper::Error),
    Body,
    MediaType(String),
    RequestId,
    Protocol(WireError),
    Product(Box<ProductError>),
    UnexpectedResponse,
    OutcomeUnknown(u128),
    NonStrictCommit,
    Clock,
}

impl PartialEq for TransportError {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (Self::Timeout, Self::Timeout))
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonLoopbackEndpoint(_) => "Hyphae endpoint is not loopback",
            Self::Timeout => "Hyphae request timed out",
            Self::Io(_) => "Hyphae connection failed",
            Self::Http(_) | Self::Body => "Hyphae HTTP exchange failed",
            Self::MediaType(_) => "Hyphae response media type differs",
            Self::RequestId => "Hyphae response request identity differs",
            Self::Protocol(_) => "Hyphae response protocol is invalid",
            Self::Product(_) => "Hyphae rejected the product operation",
            Self::UnexpectedResponse => "Hyphae returned the wrong response kind",
            Self::OutcomeUnknown(_) => "Hyphae mutation outcome requires resolution",
            Self::NonStrictCommit => "Hyphae mutation was not strictly durable",
            Self::Clock => "system clock cannot produce a Hyphae deadline",
        })
    }
}

impl std::error::Error for TransportError {}

impl From<WireError> for TransportError {
    fn from(error: WireError) -> Self {
        Self::Protocol(error)
    }
}

impl From<ProductError> for TransportError {
    fn from(error: ProductError) -> Self {
        Self::Product(Box::new(error))
    }
}

fn validate_request_id(headers: &HeaderMap, expected: u128) -> Result<(), TransportError> {
    let mut values = headers.get_all("x-hyphae-request-id").iter();
    let actual = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u128>().ok());
    if values.next().is_some() || actual != Some(expected) {
        return Err(TransportError::RequestId);
    }
    Ok(())
}

fn unix_micros() -> Result<i64, TransportError> {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| TransportError::Clock)?
        .as_micros();
    i64::try_from(micros).map_err(|_| TransportError::Clock)
}
