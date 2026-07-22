// SPDX-License-Identifier: Apache-2.0

use crate::{
    ActionAdmission, ActionContentEncoding, ActionFailure, ActionMediaType, ActionNavigation,
    ActionPolicy, ActionResponse, Body, CsrfManager, CsrfToken, HandlerError, MultipartForm,
    MultipartPolicy, Request, RequestContext, Response, RuntimeDiagnostic, SessionToken,
    StatusCode, UploadError,
};
use flate2::read::MultiGzDecoder;
use http::header::{CONTENT_ENCODING, CONTENT_TYPE, LOCATION, ORIGIN};
use http_body_util::BodyExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::Read;
use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionRequestSecurity {
    expected_origin: String,
    authenticated: bool,
    authorized: bool,
    csrf_verified: bool,
}

impl ActionRequestSecurity {
    pub fn new(expected_origin: impl Into<String>) -> Result<Self, HandlerError> {
        let expected_origin = expected_origin.into();
        let uri = http::Uri::from_str(&expected_origin).map_err(|_| {
            action_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "PLG-ACT-001",
                "configured action origin is invalid",
            )
        })?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.authority().is_none()
            || uri.path() != "/"
            || uri.query().is_some()
            || expected_origin.ends_with('/')
        {
            return Err(action_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "PLG-ACT-001",
                "configured action origin must be an absolute HTTP origin without a trailing slash",
            ));
        }
        Ok(Self {
            expected_origin,
            authenticated: false,
            authorized: false,
            csrf_verified: false,
        })
    }

    pub fn authenticated(mut self, verified: bool) -> Self {
        self.authenticated = verified;
        self
    }

    pub fn authorized(mut self, verified: bool) -> Self {
        self.authorized = verified;
        self
    }

    pub fn csrf_verified(mut self, verified: bool) -> Self {
        self.csrf_verified = verified;
        self
    }

    pub fn session_bound_csrf(
        mut self,
        manager: &CsrfManager,
        token: &CsrfToken,
        session: &SessionToken,
        policy: &ActionPolicy,
    ) -> Result<Self, HandlerError> {
        self.csrf_verified = manager
            .verify(token, session, policy.id(), policy.semantic_revision())
            .map_err(|error| {
                action_http_error(
                    StatusCode::FORBIDDEN,
                    error.code(),
                    "session-bound CSRF proof is invalid",
                )
            })?;
        Ok(self)
    }
}

#[derive(Clone, Copy)]
pub struct SessionCsrfContext<'a> {
    manager: &'a CsrfManager,
    session: &'a SessionToken,
}

impl<'a> SessionCsrfContext<'a> {
    pub fn new(manager: &'a CsrfManager, session: &'a SessionToken) -> Self {
        Self { manager, session }
    }

    fn verify(&self, token: &CsrfToken, policy: &ActionPolicy) -> Result<bool, HandlerError> {
        self.manager
            .verify(token, self.session, policy.id(), policy.semantic_revision())
            .map_err(|error| {
                action_http_error(
                    StatusCode::FORBIDDEN,
                    error.code(),
                    "session-bound CSRF proof could not be verified",
                )
            })
    }
}

pub async fn decode_action_request<Input>(
    context: &RequestContext,
    policy: &ActionPolicy,
    request: Request<Body>,
    security: &ActionRequestSecurity,
) -> Result<(Input, ActionAdmission), HandlerError>
where
    Input: DeserializeOwned,
{
    if context.action_policy(policy.id()) != Some(policy) {
        return Err(action_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PLG-ACT-001",
            "action policy is not the policy sealed into the runtime",
        ));
    }
    if request.method() != http::Method::POST {
        return Err(action_http_error(
            StatusCode::METHOD_NOT_ALLOWED,
            "PLG-ACT-102",
            "progressive actions require POST",
        ));
    }
    let media_type = admitted_media_type(request.headers().get(CONTENT_TYPE))?;
    if !policy.accepts(media_type) {
        return Err(action_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "PLG-ACT-103",
            "request media type is not declared by the action",
        ));
    }
    let content_encoding = admitted_content_encoding(request.headers())?;
    if !policy.accepts_content_encoding(content_encoding) {
        return Err(action_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "PLG-ACT-103",
            "request content encoding is not declared by the action",
        ));
    }
    let same_origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == security.expected_origin);
    let encoded = request
        .into_body()
        .collect()
        .await
        .map_err(|_| {
            action_http_error(
                StatusCode::BAD_REQUEST,
                "PLG-ACT-104",
                "action body could not be read",
            )
        })?
        .to_bytes();
    if encoded.len() > policy.max_encoded_bytes_value() {
        return Err(action_http_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "PLG-ACT-105",
            "action body exceeds its encoded-byte policy",
        ));
    }
    let body = decode_body(
        content_encoding,
        encoded.to_vec(),
        policy.max_decoded_bytes_value(),
    )
    .await?;
    if media_type == ActionMediaType::FormUrlencoded {
        let fields = if body.is_empty() {
            0
        } else {
            body.iter().filter(|byte| **byte == b'&').count() + 1
        };
        if fields > policy.max_form_fields_value() {
            return Err(action_http_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "PLG-ACT-105",
                "action form exceeds its field-count policy",
            ));
        }
    }
    let input = match media_type {
        ActionMediaType::FormUrlencoded => serde_urlencoded::from_bytes(&body).map_err(|_| ()),
        ActionMediaType::Json => serde_json::from_slice(&body).map_err(|_| ()),
        ActionMediaType::MultipartFormData => {
            return Err(action_http_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "PLG-ACT-103",
                "multipart actions require the bounded multipart decoder",
            ));
        }
    }
    .map_err(|_| {
        action_http_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "PLG-ACT-201",
            "action input does not match its declared schema",
        )
    })?;
    let admission = ActionAdmission::new(media_type, body.len())
        .same_origin(same_origin)
        .csrf_verified(security.csrf_verified)
        .authenticated(security.authenticated)
        .authorized(security.authorized);
    Ok((input, admission))
}

pub async fn decode_session_action_request<Input, Extractor>(
    context: &RequestContext,
    policy: &ActionPolicy,
    request: Request<Body>,
    security: &ActionRequestSecurity,
    csrf: SessionCsrfContext<'_>,
    extract_token: Extractor,
) -> Result<(Input, ActionAdmission), HandlerError>
where
    Input: DeserializeOwned,
    Extractor: FnOnce(&Input) -> Option<String>,
{
    let (input, admission) = decode_action_request(context, policy, request, security).await?;
    let token = extract_token(&input).ok_or_else(|| {
        action_http_error(
            StatusCode::FORBIDDEN,
            "PLG-CSRF-101",
            "session-bound CSRF proof is missing",
        )
    })?;
    let token = CsrfToken::parse(&token).map_err(|error| {
        action_http_error(
            StatusCode::FORBIDDEN,
            error.code(),
            "session-bound CSRF proof is invalid",
        )
    })?;
    let verified = csrf.verify(&token, policy)?;
    Ok((input, admission.csrf_verified(verified)))
}

pub async fn decode_multipart_action_request(
    context: &RequestContext,
    policy: &ActionPolicy,
    multipart_policy: &MultipartPolicy,
    request: Request<Body>,
    security: &ActionRequestSecurity,
) -> Result<(MultipartForm, ActionAdmission), HandlerError> {
    if context.action_policy(policy.id()) != Some(policy) {
        return Err(action_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PLG-ACT-001",
            "multipart action policy is not the policy sealed into the runtime",
        ));
    }
    if request.method() != http::Method::POST {
        return Err(action_http_error(
            StatusCode::METHOD_NOT_ALLOWED,
            "PLG-ACT-102",
            "progressive actions require POST",
        ));
    }
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            action_http_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "PLG-ACT-103",
                "multipart Content-Type is missing or invalid",
            )
        })?
        .to_owned();
    if admitted_media_type(request.headers().get(CONTENT_TYPE))?
        != ActionMediaType::MultipartFormData
        || !policy.accepts(ActionMediaType::MultipartFormData)
    {
        return Err(action_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "PLG-ACT-103",
            "multipart media type is not declared by the action",
        ));
    }
    let encoding = admitted_content_encoding(request.headers())?;
    if encoding != ActionContentEncoding::Identity
        || !policy.accepts_content_encoding(ActionContentEncoding::Identity)
    {
        return Err(action_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "PLG-ACT-103",
            "multipart actions accept only identity content encoding",
        ));
    }
    let same_origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == security.expected_origin);
    let maximum = multipart_policy
        .max_total_bytes()
        .min(policy.max_encoded_bytes_value())
        .min(policy.max_decoded_bytes_value());
    let form = crate::upload::parse_multipart(
        context.data(),
        request.into_body(),
        &content_type,
        multipart_policy,
        maximum,
    )
    .await
    .map_err(upload_handler_error)?;
    let admission = ActionAdmission::new(ActionMediaType::MultipartFormData, form.decoded_bytes())
        .same_origin(same_origin)
        .csrf_verified(security.csrf_verified)
        .authenticated(security.authenticated)
        .authorized(security.authorized);
    Ok((form, admission))
}

pub async fn decode_session_multipart_action_request(
    context: &RequestContext,
    policy: &ActionPolicy,
    multipart_policy: &MultipartPolicy,
    request: Request<Body>,
    security: &ActionRequestSecurity,
    csrf: SessionCsrfContext<'_>,
    csrf_field: &str,
) -> Result<(MultipartForm, ActionAdmission), HandlerError> {
    let (form, admission) =
        decode_multipart_action_request(context, policy, multipart_policy, request, security)
            .await?;
    let token = form.text(csrf_field).ok_or_else(|| {
        action_http_error(
            StatusCode::FORBIDDEN,
            "PLG-CSRF-101",
            "session-bound CSRF proof is missing",
        )
    })?;
    let token = CsrfToken::parse(token).map_err(|error| {
        action_http_error(
            StatusCode::FORBIDDEN,
            error.code(),
            "session-bound CSRF proof is invalid",
        )
    })?;
    let verified = csrf.verify(&token, policy)?;
    Ok((form, admission.csrf_verified(verified)))
}

fn upload_handler_error(error: UploadError) -> HandlerError {
    let status = match error {
        UploadError::PartLimit
        | UploadError::FileLimit
        | UploadError::TextLimit
        | UploadError::TotalLimit => StatusCode::PAYLOAD_TOO_LARGE,
        UploadError::InvalidBoundary | UploadError::Malformed | UploadError::MissingFieldName => {
            StatusCode::BAD_REQUEST
        }
        UploadError::UnknownField
        | UploadError::FieldKindMismatch
        | UploadError::InvalidFilename
        | UploadError::InvalidText => StatusCode::UNPROCESSABLE_ENTITY,
        UploadError::Cancelled => StatusCode::REQUEST_TIMEOUT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    action_http_error(status, error.code(), "multipart action input was rejected")
}

fn admitted_content_encoding(
    headers: &http::HeaderMap,
) -> Result<ActionContentEncoding, HandlerError> {
    let values = headers
        .get_all(CONTENT_ENCODING)
        .iter()
        .map(|value| value.to_str().ok())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            action_http_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "PLG-ACT-103",
                "action Content-Encoding is invalid",
            )
        })?;
    if values.is_empty() {
        return Ok(ActionContentEncoding::Identity);
    }
    let encodings = values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .collect::<Vec<_>>();
    if encodings.len() != 1 {
        return Err(action_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "PLG-ACT-103",
            "stacked action content encodings are unsupported",
        ));
    }
    if encodings[0].eq_ignore_ascii_case("identity") {
        Ok(ActionContentEncoding::Identity)
    } else if encodings[0].eq_ignore_ascii_case("gzip") {
        Ok(ActionContentEncoding::Gzip)
    } else {
        Err(action_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "PLG-ACT-103",
            "action Content-Encoding is unsupported",
        ))
    }
}

async fn decode_body(
    encoding: ActionContentEncoding,
    encoded: Vec<u8>,
    maximum: usize,
) -> Result<Vec<u8>, HandlerError> {
    if encoding == ActionContentEncoding::Identity {
        if encoded.len() > maximum {
            return Err(action_http_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "PLG-ACT-105",
                "action body exceeds its decoded-byte policy",
            ));
        }
        return Ok(encoded);
    }
    let decoded = tokio::task::spawn_blocking(move || {
        let mut decoder = MultiGzDecoder::new(encoded.as_slice()).take((maximum as u64) + 1);
        let mut decoded = Vec::with_capacity(maximum.min(64 * 1_024));
        decoder.read_to_end(&mut decoded).map(|_| decoded)
    })
    .await
    .map_err(|_| {
        action_http_error(
            StatusCode::BAD_REQUEST,
            "PLG-ACT-104",
            "action decompression task failed",
        )
    })?
    .map_err(|_| {
        action_http_error(
            StatusCode::BAD_REQUEST,
            "PLG-ACT-104",
            "action gzip body is invalid",
        )
    })?;
    if decoded.len() > maximum {
        return Err(action_http_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "PLG-ACT-105",
            "action body exceeds its decoded-byte policy",
        ));
    }
    Ok(decoded)
}

fn admitted_media_type(value: Option<&http::HeaderValue>) -> Result<ActionMediaType, HandlerError> {
    let value = value.and_then(|value| value.to_str().ok()).ok_or_else(|| {
        action_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "PLG-ACT-103",
            "action Content-Type is missing or invalid",
        )
    })?;
    let media_type = value.split(';').next().unwrap_or_default().trim();
    if media_type.eq_ignore_ascii_case(ActionMediaType::FormUrlencoded.as_str()) {
        Ok(ActionMediaType::FormUrlencoded)
    } else if media_type.eq_ignore_ascii_case(ActionMediaType::Json.as_str()) {
        Ok(ActionMediaType::Json)
    } else if media_type.eq_ignore_ascii_case(ActionMediaType::MultipartFormData.as_str()) {
        Ok(ActionMediaType::MultipartFormData)
    } else {
        Err(action_http_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "PLG-ACT-103",
            "action Content-Type is unsupported",
        ))
    }
}

pub fn action_failure_to_handler_error(failure: ActionFailure) -> HandlerError {
    let status = match failure.error() {
        crate::DataError::ActionAdmission(_) => StatusCode::FORBIDDEN,
        crate::DataError::ActionInput(_) => StatusCode::UNPROCESSABLE_ENTITY,
        crate::DataError::Cancelled | crate::DataError::Deadline => StatusCode::REQUEST_TIMEOUT,
        crate::DataError::ActionOutcomeUnknown | crate::DataError::ActionIdempotencyConflict => {
            StatusCode::CONFLICT
        }
        crate::DataError::ActionInProgress => StatusCode::TOO_EARLY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    HandlerError::new(
        status,
        RuntimeDiagnostic::new(failure.error().code(), bounded(&failure.to_string(), 320))
            .expect("action failure diagnostics are bounded"),
    )
}

pub fn progressive_action_response<Output, FieldErrors>(
    response: &ActionResponse<Output, FieldErrors>,
) -> Result<Response<Body>, HandlerError>
where
    Output: Serialize,
    FieldErrors: Serialize,
{
    match response {
        ActionResponse::Success {
            navigation: ActionNavigation::SeeOther(location),
            ..
        } => {
            let location = http::HeaderValue::from_str(location).map_err(|_| {
                action_http_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PLG-ACT-302",
                    "action navigation contains an invalid Location value",
                )
            })?;
            Response::builder()
                .status(StatusCode::SEE_OTHER)
                .header(LOCATION, location)
                .body(Body::empty())
                .map_err(|_| {
                    action_http_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "PLG-ACT-500",
                        "action response could not be built",
                    )
                })
        }
        ActionResponse::Success {
            navigation: ActionNavigation::Stay,
            ..
        } => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .map_err(|_| {
                action_http_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PLG-ACT-500",
                    "action response could not be built",
                )
            }),
        ActionResponse::Invalid { field_errors } => {
            let body = serde_json::to_vec(field_errors).map_err(|_| {
                action_http_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PLG-ACT-500",
                    "field errors could not be serialized",
                )
            })?;
            Response::builder()
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(body))
                .map_err(|_| {
                    action_http_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "PLG-ACT-500",
                        "action response could not be built",
                    )
                })
        }
    }
}

fn action_http_error(status: StatusCode, code: &str, message: &str) -> HandlerError {
    HandlerError::new(
        status,
        RuntimeDiagnostic::new(code, message).expect("static action diagnostic is valid"),
    )
}

fn bounded(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        value.to_owned()
    } else {
        let mut end = maximum;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value[..end].to_owned()
    }
}
