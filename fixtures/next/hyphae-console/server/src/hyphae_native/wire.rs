// SPDX-License-Identifier: AGPL-3.0-only

use std::fmt;

pub const PRODUCT_MEDIA_TYPE: &str = "application/vnd.hyphae.product-v1";
pub const ERROR_MEDIA_TYPE: &str = "application/vnd.hyphae.error-v1";
pub const MAX_WIRE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 8 * 1024;
const PRODUCT_REQUEST_MAGIC: &[u8; 8] = b"HYPREQ01";
const PRODUCT_RESPONSE_MAGIC: &[u8; 8] = b"HYPRSP01";
const PRODUCT_ERROR_MAGIC: &[u8; 8] = b"HYPERR01";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductLimits {
    pub max_count: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub max_work_units: u64,
    pub max_memory_bytes: u64,
}

impl Default for ProductLimits {
    fn default() -> Self {
        Self {
            max_count: 4_096,
            max_request_bytes: MAX_WIRE_BYTES as u64,
            max_response_bytes: MAX_WIRE_BYTES as u64,
            max_work_units: 1_000_000,
            max_memory_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductOperation<'a> {
    Get(&'a [u8]),
    Set { key: &'a [u8], value: &'a [u8] },
    TransactionStatusByIdempotency(u128),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductCapabilities {
    pub product_api_version: u16,
    pub native_directory_format: u16,
    pub logical_catalog_codec_version: u16,
    pub catalog_tree_format_version: u16,
    pub max_catalog_items: u64,
    pub max_catalog_visits: u64,
    pub max_catalog_bytes: u64,
    pub max_sql_statement_bytes: u64,
    pub max_sql_parameters: u64,
    pub max_sql_rows: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProductResponse {
    Capabilities(ProductCapabilities),
    Value(Option<Vec<u8>>),
    Commit(CommitOutcome),
    TransactionStatus(TransactionStatus),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    Committed(CommitReceipt),
    OutcomeUnknown(u128),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitReceipt {
    pub transaction_id: u128,
    pub durability: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionStatus {
    Unknown,
    Committed(CommitReceipt),
    RolledBack(u128),
    OutcomeUnknown(u128),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCategory {
    InvalidRequest,
    NotFound,
    Conflict,
    Limit,
    Deadline,
    Cancelled,
    Authorization,
    Corruption,
    Unavailable,
    Io,
    Internal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryClass {
    Never,
    SameRequest,
    NewSnapshot,
    AfterBackoff,
    AfterRecovery,
    UnknownCommit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionState {
    None,
    Active,
    RolledBack,
    Committed,
    OutcomeUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownDetail {
    pub tag: u16,
    pub value: Vec<u8>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ProductError {
    pub code: String,
    pub category: ErrorCategory,
    pub retry: RetryClass,
    pub transaction_state: TransactionState,
    pub message: String,
    pub request_id: Option<u128>,
    pub transaction_id: Option<u128>,
    pub unknown_details: Vec<UnknownDetail>,
}

impl fmt::Debug for ProductError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductError")
            .field("code", &self.code)
            .field("category", &self.category)
            .field("retry", &self.retry)
            .field("transaction_state", &self.transaction_state)
            .field("request_id", &self.request_id)
            .field("transaction_id", &self.transaction_id)
            .field("unknown_detail_count", &self.unknown_details.len())
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WireError {
    Truncated,
    Malformed,
    Bound,
    Utf8,
    Unsupported,
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Truncated => "Hyphae wire value is truncated",
            Self::Malformed => "Hyphae wire value is malformed",
            Self::Bound => "Hyphae wire value exceeds a bound",
            Self::Utf8 => "Hyphae wire value contains invalid UTF-8",
            Self::Unsupported => "Hyphae wire value uses an unsupported field",
        })
    }
}

impl std::error::Error for WireError {}

pub fn encode_request(
    operation: ProductOperation<'_>,
    logical_time_micros: i64,
    deadline_micros: Option<i64>,
    idempotency_token: Option<u128>,
    limits: ProductLimits,
) -> Result<Vec<u8>, WireError> {
    validate_limits(limits)?;
    if idempotency_token == Some(0) {
        return Err(WireError::Malformed);
    }
    let (tag, body) = encode_operation(operation)?;
    let mut payload = Vec::with_capacity(96 + body.len());
    payload.extend_from_slice(&logical_time_micros.to_le_bytes());
    payload.extend_from_slice(&deadline_micros.unwrap_or(0).to_le_bytes());
    if let Some(token) = idempotency_token {
        payload.extend_from_slice(&token.to_le_bytes());
    }
    for limit in [
        limits.max_count,
        limits.max_request_bytes,
        limits.max_response_bytes,
        limits.max_work_units,
        limits.max_memory_bytes,
    ] {
        payload.extend_from_slice(&limit.to_le_bytes());
    }
    payload.push(0);
    payload.extend_from_slice(if idempotency_token.is_some() {
        &[1, 0, 0, 0, 0, 0, 0]
    } else {
        &[0; 7]
    });
    payload.extend_from_slice(&body);
    envelope(PRODUCT_REQUEST_MAGIC, tag, &payload)
}

pub fn decode_response(encoded: &[u8]) -> Result<ProductResponse, WireError> {
    let (tag, payload) = decode_envelope(encoded, PRODUCT_RESPONSE_MAGIC, MAX_WIRE_BYTES)?;
    let mut decoder = Decoder::new(payload);
    let response = match tag {
        1 => ProductResponse::Capabilities(ProductCapabilities {
            product_api_version: decoder.u16()?,
            native_directory_format: decoder.u16()?,
            logical_catalog_codec_version: decoder.u16()?,
            catalog_tree_format_version: decoder.u16()?,
            max_catalog_items: decoder.u64()?,
            max_catalog_visits: decoder.u64()?,
            max_catalog_bytes: decoder.u64()?,
            max_sql_statement_bytes: decoder.u64()?,
            max_sql_parameters: decoder.u64()?,
            max_sql_rows: decoder.u64()?,
        }),
        4 => {
            let present = decoder.u8()?;
            if present > 1 || decoder.bytes(3)? != [0; 3] {
                return Err(WireError::Malformed);
            }
            ProductResponse::Value(if present == 1 {
                Some(decoder.owned_bytes(MAX_WIRE_BYTES)?)
            } else {
                None
            })
        }
        5 => ProductResponse::Commit(decode_commit_outcome(&mut decoder)?),
        7 => ProductResponse::TransactionStatus(decode_transaction_status(&mut decoder)?),
        _ => return Err(WireError::Unsupported),
    };
    if !decoder.is_empty() {
        return Err(WireError::Malformed);
    }
    Ok(response)
}

pub fn decode_error(encoded: &[u8]) -> Result<ProductError, WireError> {
    if encoded.len() < 20 {
        return Err(WireError::Truncated);
    }
    if encoded.len() > MAX_ERROR_BYTES
        || &encoded[..8] != PRODUCT_ERROR_MAGIC
        || read_u32(&encoded[8..12]) as usize != encoded.len()
    {
        return Err(WireError::Malformed);
    }
    let category = decode_category(encoded[12])?;
    let retry = decode_retry(encoded[13])?;
    let transaction_state = decode_transaction_state(encoded[14])?;
    let flags = encoded[15];
    if flags & !0x1f != 0 {
        return Err(WireError::Unsupported);
    }
    let code_length = usize::from(encoded[16]);
    let message_length = usize::from(read_u16(&encoded[17..19]));
    let detail_count = usize::from(encoded[19]);
    if code_length == 0
        || code_length > 64
        || message_length == 0
        || message_length > 256
        || detail_count > 18
    {
        return Err(WireError::Bound);
    }
    let mut decoder = Decoder::new(&encoded[20..]);
    let code = decoder.text(code_length)?.to_owned();
    validate_identifier(&code)?;
    let message = decoder.text(message_length)?.to_owned();
    let request_id = if flags & 1 != 0 {
        Some(nonzero(decoder.u128()?)?)
    } else {
        None
    };
    if flags & 2 != 0 {
        nonzero(decoder.u128()?)?;
    }
    if flags & 4 != 0 {
        nonzero(decoder.u128()?)?;
    }
    if flags & 8 != 0 {
        let length = usize::from(decoder.u8()?);
        if length == 0 || length > 64 {
            return Err(WireError::Bound);
        }
        validate_identifier(decoder.text(length)?)?;
        decoder.u64()?;
        decoder.u64()?;
    }
    if flags & 16 != 0 {
        let start = decoder.u32()?;
        let end = decoder.u32()?;
        if start > end {
            return Err(WireError::Malformed);
        }
    }
    let mut transaction_id = None;
    let mut unknown_details = Vec::new();
    let mut previous_tag = 0;
    for _ in 0..detail_count {
        let tag = decoder.u16()?;
        let length = usize::from(decoder.u16()?);
        if tag == 0 || tag <= previous_tag || length > 256 {
            return Err(WireError::Malformed);
        }
        previous_tag = tag;
        let value = decoder.bytes(length)?;
        match tag {
            1 if length == 8 => {
                std::str::from_utf8(value).map_err(|_| WireError::Utf8)?;
            }
            2 if length == 16 => transaction_id = Some(nonzero(read_u128(value))?),
            1 | 2 => return Err(WireError::Malformed),
            _ => unknown_details.push(UnknownDetail {
                tag,
                value: value.to_vec(),
            }),
        }
    }
    if !decoder.is_empty() {
        return Err(WireError::Malformed);
    }
    Ok(ProductError {
        code,
        category,
        retry,
        transaction_state,
        message,
        request_id,
        transaction_id,
        unknown_details,
    })
}

fn encode_operation(operation: ProductOperation<'_>) -> Result<(u16, Vec<u8>), WireError> {
    let mut body = Vec::new();
    let tag = match operation {
        ProductOperation::Get(key) => {
            put_bytes(&mut body, key)?;
            5
        }
        ProductOperation::Set { key, value } => {
            put_bytes(&mut body, key)?;
            put_bytes(&mut body, value)?;
            body.extend_from_slice(&[0; 8]);
            6
        }
        ProductOperation::TransactionStatusByIdempotency(token) => {
            body.extend_from_slice(&nonzero(token)?.to_le_bytes());
            39
        }
    };
    Ok((tag, body))
}

fn decode_commit_outcome(decoder: &mut Decoder<'_>) -> Result<CommitOutcome, WireError> {
    let tag = decoder.u8()?;
    if decoder.bytes(7)? != [0; 7] {
        return Err(WireError::Malformed);
    }
    match tag {
        0 => Ok(CommitOutcome::Committed(decode_receipt(decoder)?)),
        1 => Ok(CommitOutcome::OutcomeUnknown(nonzero(decoder.u128()?)?)),
        _ => Err(WireError::Malformed),
    }
}

fn decode_receipt(decoder: &mut Decoder<'_>) -> Result<CommitReceipt, WireError> {
    let transaction_id = nonzero(decoder.u128()?)?;
    decoder.u64()?;
    decoder.u64()?;
    decoder.u64()?;
    decoder.bytes(32)?;
    let durability = decoder.u8()?;
    if durability > 2 || decoder.bytes(7)? != [0; 7] {
        return Err(WireError::Malformed);
    }
    decoder.u64()?;
    decoder.u64()?;
    Ok(CommitReceipt {
        transaction_id,
        durability,
    })
}

fn decode_transaction_status(decoder: &mut Decoder<'_>) -> Result<TransactionStatus, WireError> {
    match decoder.u8()? {
        0 => Ok(TransactionStatus::Unknown),
        1 => Ok(TransactionStatus::Committed(decode_receipt(decoder)?)),
        2 => Ok(TransactionStatus::RolledBack(nonzero(decoder.u128()?)?)),
        3 => Ok(TransactionStatus::OutcomeUnknown(nonzero(decoder.u128()?)?)),
        _ => Err(WireError::Malformed),
    }
}

fn envelope(magic: &[u8; 8], tag: u16, payload: &[u8]) -> Result<Vec<u8>, WireError> {
    let length = 16_usize
        .checked_add(payload.len())
        .ok_or(WireError::Bound)?;
    if length > MAX_WIRE_BYTES {
        return Err(WireError::Bound);
    }
    let mut encoded = Vec::with_capacity(length);
    encoded.extend_from_slice(magic);
    encoded.extend_from_slice(&(length as u32).to_le_bytes());
    encoded.extend_from_slice(&tag.to_le_bytes());
    encoded.extend_from_slice(&0_u16.to_le_bytes());
    encoded.extend_from_slice(payload);
    Ok(encoded)
}

fn decode_envelope<'a>(
    encoded: &'a [u8],
    magic: &[u8; 8],
    maximum: usize,
) -> Result<(u16, &'a [u8]), WireError> {
    if encoded.len() < 16 {
        return Err(WireError::Truncated);
    }
    if encoded.len() > maximum
        || &encoded[..8] != magic
        || read_u32(&encoded[8..12]) as usize != encoded.len()
        || read_u16(&encoded[14..16]) != 0
    {
        return Err(WireError::Malformed);
    }
    Ok((read_u16(&encoded[12..14]), &encoded[16..]))
}

fn validate_limits(limits: ProductLimits) -> Result<(), WireError> {
    if limits.max_count == 0
        || limits.max_request_bytes == 0
        || limits.max_response_bytes == 0
        || limits.max_work_units == 0
        || limits.max_memory_bytes == 0
        || limits.max_request_bytes > MAX_WIRE_BYTES as u64
        || limits.max_response_bytes > MAX_WIRE_BYTES as u64
    {
        return Err(WireError::Bound);
    }
    Ok(())
}

fn put_bytes(encoded: &mut Vec<u8>, value: &[u8]) -> Result<(), WireError> {
    let length = u32::try_from(value.len()).map_err(|_| WireError::Bound)?;
    encoded.extend_from_slice(&length.to_le_bytes());
    encoded.extend_from_slice(value);
    Ok(())
}

fn decode_category(tag: u8) -> Result<ErrorCategory, WireError> {
    Ok(match tag {
        0 => ErrorCategory::InvalidRequest,
        1 => ErrorCategory::NotFound,
        2 => ErrorCategory::Conflict,
        3 => ErrorCategory::Limit,
        4 => ErrorCategory::Deadline,
        5 => ErrorCategory::Cancelled,
        6 => ErrorCategory::Authorization,
        7 => ErrorCategory::Corruption,
        8 => ErrorCategory::Unavailable,
        9 => ErrorCategory::Io,
        10 => ErrorCategory::Internal,
        _ => return Err(WireError::Unsupported),
    })
}
fn decode_retry(tag: u8) -> Result<RetryClass, WireError> {
    Ok(match tag {
        0 => RetryClass::Never,
        1 => RetryClass::SameRequest,
        2 => RetryClass::NewSnapshot,
        3 => RetryClass::AfterBackoff,
        4 => RetryClass::AfterRecovery,
        5 => RetryClass::UnknownCommit,
        _ => return Err(WireError::Unsupported),
    })
}
fn decode_transaction_state(tag: u8) -> Result<TransactionState, WireError> {
    Ok(match tag {
        0 => TransactionState::None,
        1 => TransactionState::Active,
        2 => TransactionState::RolledBack,
        3 => TransactionState::Committed,
        4 => TransactionState::OutcomeUnknown,
        _ => return Err(WireError::Unsupported),
    })
}
fn nonzero(value: u128) -> Result<u128, WireError> {
    if value == 0 {
        Err(WireError::Malformed)
    } else {
        Ok(value)
    }
}

fn validate_identifier(value: &str) -> Result<(), WireError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(WireError::Malformed);
    }
    Ok(())
}
fn read_u16(value: &[u8]) -> u16 {
    u16::from_le_bytes(value[..2].try_into().expect("two bytes"))
}
fn read_u32(value: &[u8]) -> u32 {
    u32::from_le_bytes(value[..4].try_into().expect("four bytes"))
}
fn read_u128(value: &[u8]) -> u128 {
    u128::from_le_bytes(value[..16].try_into().expect("sixteen bytes"))
}

struct Decoder<'a> {
    remaining: &'a [u8],
}
impl<'a> Decoder<'a> {
    const fn new(remaining: &'a [u8]) -> Self {
        Self { remaining }
    }
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], WireError> {
        if self.remaining.len() < length {
            return Err(WireError::Truncated);
        }
        let (value, rest) = self.remaining.split_at(length);
        self.remaining = rest;
        Ok(value)
    }
    fn text(&mut self, length: usize) -> Result<&'a str, WireError> {
        std::str::from_utf8(self.bytes(length)?).map_err(|_| WireError::Utf8)
    }
    fn owned_bytes(&mut self, maximum: usize) -> Result<Vec<u8>, WireError> {
        let length = self.u32()? as usize;
        if length > maximum {
            return Err(WireError::Bound);
        }
        Ok(self.bytes(length)?.to_vec())
    }
    fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, WireError> {
        Ok(read_u16(self.bytes(2)?))
    }
    fn u32(&mut self) -> Result<u32, WireError> {
        Ok(read_u32(self.bytes(4)?))
    }
    fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(
            self.bytes(8)?.try_into().expect("eight bytes"),
        ))
    }
    fn u128(&mut self) -> Result<u128, WireError> {
        Ok(read_u128(self.bytes(16)?))
    }
    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}
