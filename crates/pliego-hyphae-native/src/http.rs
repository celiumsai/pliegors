// SPDX-License-Identifier: AGPL-3.0-only

use crate::admit;
use crate::error::{TransportError, TransportErrorKind};
use crate::wire::{
    CommitOutcome, ERROR_MEDIA_TYPE, MAX_WIRE_BYTES, PRODUCT_MEDIA_TYPE, ProductCapabilities,
    ProductLimits, ProductOperation, ProductResponse, TransactionStatus, WireError, decode_error,
    decode_response, encode_request,
};
use bytes::Bytes;
use http::{HeaderMap, Request, StatusCode};
use http_body_util::{BodyExt, Empty, Full, Limited};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;

/// Maximum scalar value accepted before any HTTP call is attempted.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
/// A bounded HTTP `/v2` client that accepts only loopback endpoints.
pub struct NativeHttpClient {
    endpoint: SocketAddr,
    timeout: Duration,
}

impl NativeHttpClient {
    /// Creates a client for one loopback sidecar endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is not loopback or `timeout` is zero.
    pub fn new(endpoint: SocketAddr, timeout: Duration) -> Result<Self, TransportError> {
        if !endpoint.ip().is_loopback() {
            return Err(TransportErrorKind::NonLoopbackEndpoint(endpoint).into());
        }
        if timeout.is_zero() {
            return Err(TransportErrorKind::Timeout.into());
        }
        Ok(Self { endpoint, timeout })
    }

    pub(crate) async fn validate_capabilities(
        &self,
        request_id: u128,
    ) -> Result<ProductCapabilities, TransportError> {
        admit::capabilities(self.capabilities(request_id).await?)
    }

    /// Reads the pinned Product capability envelope.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid request identity, transport failure,
    /// unexpected media type, malformed envelopes, or product rejection.
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
            .map_err(|_| TransportErrorKind::Protocol(WireError::Malformed))?;
        match self.send(request, request_id).await? {
            ProductResponse::Capabilities(capabilities) => Ok(capabilities),
            _ => Err(TransportErrorKind::UnexpectedResponse.into()),
        }
    }

    /// Reads one opaque scalar value by product key.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid request identity, transport failure,
    /// malformed responses, or product rejection.
    pub async fn get(
        &self,
        key: &[u8],
        request_id: u128,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        match self
            .execute(ProductOperation::Get(key), None, request_id)
            .await?
        {
            ProductResponse::Value(value) => Ok(value),
            _ => Err(TransportErrorKind::UnexpectedResponse.into()),
        }
    }

    /// Strictly persists one opaque scalar value.
    ///
    /// Uncertain acknowledgements are resolved with the original nonzero
    /// idempotency token. This method never retries the semantic write.
    ///
    /// # Errors
    ///
    /// Returns an error for values above [`MAX_VALUE_BYTES`], a zero token,
    /// transport or product failure, non-strict durability, rollback, or an
    /// outcome that cannot be proven committed.
    pub async fn set(
        &self,
        key: &[u8],
        value: &[u8],
        idempotency_token: u128,
        request_id: u128,
    ) -> Result<(), TransportError> {
        if value.len() > MAX_VALUE_BYTES {
            return Err(TransportErrorKind::ValueTooLarge {
                actual: value.len(),
                maximum: MAX_VALUE_BYTES,
            }
            .into());
        }
        if idempotency_token == 0 {
            return Err(TransportErrorKind::InvalidIdempotencyToken.into());
        }
        let result = self
            .execute(
                ProductOperation::Set { key, value },
                Some(idempotency_token),
                request_id,
            )
            .await;
        match result {
            Err(error) => {
                let Some(expected_transaction) = error.mutation_resolution() else {
                    return Err(error);
                };
                self.resolve_mutation(
                    idempotency_token,
                    expected_transaction,
                    next_request_id(request_id)?,
                )
                .await
            }
            Ok(ProductResponse::Commit(CommitOutcome::Committed(receipt)))
                if receipt.durability == 0 =>
            {
                Ok(())
            }
            Ok(ProductResponse::Commit(CommitOutcome::OutcomeUnknown(transaction_id))) => {
                self.resolve_mutation(
                    idempotency_token,
                    Some(transaction_id),
                    next_request_id(request_id)?,
                )
                .await
            }
            Ok(ProductResponse::Commit(_)) => Err(TransportErrorKind::NonStrictCommit.into()),
            Ok(_) => Err(TransportErrorKind::UnexpectedResponse.into()),
        }
    }

    /// Reads the product transaction status for an idempotency token.
    ///
    /// # Errors
    ///
    /// Returns an error for a zero token, invalid request identity, transport
    /// failure, malformed response, or product rejection.
    pub async fn resolve_idempotency(
        &self,
        idempotency_token: u128,
        request_id: u128,
    ) -> Result<TransactionStatus, TransportError> {
        if idempotency_token == 0 {
            return Err(TransportErrorKind::InvalidIdempotencyToken.into());
        }
        match self
            .execute(
                ProductOperation::TransactionStatusByIdempotency(idempotency_token),
                None,
                request_id,
            )
            .await?
        {
            ProductResponse::TransactionStatus(status) => Ok(status),
            _ => Err(TransportErrorKind::UnexpectedResponse.into()),
        }
    }

    async fn resolve_mutation(
        &self,
        idempotency_token: u128,
        expected_transaction: Option<u128>,
        request_id: u128,
    ) -> Result<(), TransportError> {
        match self
            .resolve_idempotency(idempotency_token, request_id)
            .await
        {
            Ok(TransactionStatus::Committed {
                transaction_id,
                durability: 0,
            }) if transaction_matches(expected_transaction, transaction_id) => Ok(()),
            Ok(TransactionStatus::RolledBack(transaction_id))
                if transaction_matches(expected_transaction, transaction_id) =>
            {
                Err(TransportErrorKind::MutationRolledBack.into())
            }
            Ok(TransactionStatus::Committed {
                transaction_id,
                durability: _,
            }) if transaction_matches(expected_transaction, transaction_id) => {
                Err(TransportErrorKind::NonStrictCommit.into())
            }
            Ok(TransactionStatus::Unknown) | Err(_) => {
                Err(TransportErrorKind::OutcomeUnknown(expected_transaction).into())
            }
            Ok(TransactionStatus::OutcomeUnknown(transaction_id))
                if transaction_matches(expected_transaction, transaction_id) =>
            {
                Err(TransportErrorKind::OutcomeUnknown(Some(transaction_id)).into())
            }
            Ok(
                TransactionStatus::Committed { .. }
                | TransactionStatus::RolledBack(_)
                | TransactionStatus::OutcomeUnknown(_),
            ) => Err(TransportErrorKind::OutcomeUnknown(expected_transaction).into()),
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
                i64::try_from(self.timeout.as_micros()).map_err(|_| TransportErrorKind::Timeout)?,
            )
            .ok_or(TransportErrorKind::Timeout)?;
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
            .map_err(|_| TransportErrorKind::Protocol(WireError::Malformed))?;
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
            return Err(TransportErrorKind::RequestId.into());
        }
        let endpoint = self.endpoint;
        let future = async move {
            let stream = TcpStream::connect(endpoint)
                .await
                .map_err(TransportErrorKind::Io)?;
            let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
                .await
                .map_err(TransportErrorKind::Http)?;
            let connection = tokio::spawn(connection);
            let response = sender
                .send_request(request)
                .await
                .map_err(TransportErrorKind::Http)?;
            let status = response.status();
            validate_request_id(response.headers(), request_id)?;
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .ok_or(TransportErrorKind::MediaType)?
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned();
            let body = Limited::new(response.into_body(), MAX_WIRE_BYTES)
                .collect()
                .await
                .map_err(|_| TransportErrorKind::Body)?
                .to_bytes();
            drop(sender);
            connection.abort();
            if status == StatusCode::OK && content_type == PRODUCT_MEDIA_TYPE {
                return decode_response(&body).map_err(Into::into);
            }
            if status != StatusCode::OK && content_type == ERROR_MEDIA_TYPE {
                let error = decode_error(&body)?;
                if error.request_id.is_some_and(|actual| actual != request_id) {
                    return Err(TransportErrorKind::RequestId.into());
                }
                return Err(TransportError::product(status, error));
            }
            let _ = content_type;
            Err(TransportErrorKind::MediaType.into())
        };
        tokio::time::timeout(self.timeout, future)
            .await
            .map_err(|_| TransportError::from(TransportErrorKind::Timeout))?
    }
}

fn validate_request_id(headers: &HeaderMap, expected: u128) -> Result<(), TransportError> {
    let mut values = headers.get_all("x-hyphae-request-id").iter();
    let actual = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u128>().ok());
    if values.next().is_some() || actual != Some(expected) {
        return Err(TransportErrorKind::RequestId.into());
    }
    Ok(())
}

fn unix_micros() -> Result<i64, TransportError> {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| TransportErrorKind::Clock)?
        .as_micros();
    i64::try_from(micros).map_err(|_| TransportErrorKind::Clock.into())
}

fn next_request_id(request_id: u128) -> Result<u128, TransportError> {
    request_id
        .checked_add(1)
        .filter(|request_id| *request_id != 0)
        .ok_or_else(|| TransportErrorKind::RequestId.into())
}

fn transaction_matches(expected: Option<u128>, actual: u128) -> bool {
    expected.is_none_or(|expected| expected == actual)
}
