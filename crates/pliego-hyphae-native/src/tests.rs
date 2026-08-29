// SPDX-License-Identifier: AGPL-3.0-only

use super::keys::tenant_prefix;
use super::wire::{
    ERROR_MEDIA_TYPE, PRODUCT_MEDIA_TYPE, ProductLimits, ProductOperation, ProductResponse,
    WireError, decode_error, decode_response, encode_request,
};
use super::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const CAPABILITIES_RESPONSE: &str = "485950525350303148000000010000000100010002000600001000000000000000400000000000000000000100000000000001000000000000040000000000000004000000000000";
const GET_REQUEST: &str = "485950524551303164000000050000000080982817650600000000000000000000100000000000000000000100000000000000010000000040420f00000000000000000400000000000000000000000010000000706c6965676f72732d636f6e736f6c65";
const SET_REQUEST: &str = "485950524551303180000000060000000080982817650600000000000000000000100000000000000000000100000000000000010000000040420f00000000000000000400000000000000000000000010000000706c6965676f72732d636f6e736f6c651000000070657273697374656e742d76616c75650000000000000000";
const GET_RESPONSE: &str =
    "48595052535030312800000004000000010000001000000070657273697374656e742d76616c7565";
const UNKNOWN_ERROR: &str = "4859504552523031480000000a0400000e1f00016675747572655f6661696c7572656675747572652070726f64756374206f7065726174696f6e206661696c65642a000300010203";

#[test]
fn encodes_released_scalar_request_vectors() {
    let limits = ProductLimits::default();
    assert_eq!(
        hex(&encode_request(
            ProductOperation::Get(b"pliegors-console"),
            1_800_000_000_000_000,
            None,
            None,
            limits,
        )
        .unwrap()),
        GET_REQUEST,
    );
    assert_eq!(
        hex(&encode_request(
            ProductOperation::Set {
                key: b"pliegors-console",
                value: b"persistent-value",
            },
            1_800_000_000_000_000,
            None,
            None,
            limits,
        )
        .unwrap()),
        SET_REQUEST,
    );
}

#[test]
fn decodes_released_capability_vector() {
    assert_eq!(
        decode_response(&decode_hex(CAPABILITIES_RESPONSE)).unwrap(),
        ProductResponse::Capabilities(ProductCapabilities {
            product_api_version: 1,
            native_directory_format: 1,
            logical_catalog_codec_version: 2,
            catalog_tree_format_version: 6,
            max_catalog_items: 4_096,
            max_catalog_visits: 16_384,
            max_catalog_bytes: 16_777_216,
            max_sql_statement_bytes: 65_536,
            max_sql_parameters: 1_024,
            max_sql_rows: 1_024,
        }),
    );
    assert_eq!(
        decode_response(&decode_hex(GET_RESPONSE)).unwrap(),
        ProductResponse::Value(Some(b"persistent-value".to_vec())),
    );
}

#[test]
fn rejects_truncated_noncanonical_and_trailing_product_envelopes() {
    let valid = decode_hex(GET_RESPONSE);
    for length in 0..valid.len() {
        assert!(
            decode_response(&valid[..length]).is_err(),
            "accepted {length} bytes"
        );
    }
    let mut reserved = valid.clone();
    reserved[14] = 1;
    assert_eq!(decode_response(&reserved), Err(WireError::Malformed));
    let mut trailing = valid;
    trailing.push(0);
    assert_eq!(decode_response(&trailing), Err(WireError::Malformed));
}

#[test]
fn unknown_product_error_never_decodes_as_success() {
    let encoded = decode_hex(UNKNOWN_ERROR);
    assert!(decode_response(&encoded).is_err());
    let error = decode_error(&encoded).unwrap();
    assert_eq!(error.code, "future_failure");
    assert_eq!(error.unknown_details[0].tag, 42);
    assert_eq!(error.unknown_details[0].value, [1, 2, 3]);
}

#[test]
fn tenant_key_codec_accepts_only_bounded_lowercase_segments() {
    assert_eq!(tenant_prefix("tenant-7").unwrap(), b"pliego/tenant-7/");
    for invalid in [
        "",
        "a2345678901234567890123456789012345678901234567890123456789012345",
        "Tenant",
        "tenant/path",
        "tenant_id",
        "tenant-\u{e9}",
    ] {
        assert!(tenant_prefix(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn authority_is_pinned_to_the_reviewed_release() {
    let authority = SidecarAuthority::load().unwrap();
    assert_eq!(authority.release_tag(), "v1.0.1");
    assert_eq!(
        authority.release_revision(),
        "84161cf067141b60f4847b965ef77c5b749749c0"
    );
    assert!(authority.current_artifact().is_ok());
}

#[test]
fn installation_rejects_missing_or_changed_executable() {
    let authority = SidecarAuthority::load().unwrap();
    assert!(matches!(
        HyphaeInstallation::admit(std::path::Path::new(""), &authority),
        Err(error) if error.code() == "missing_executable"
    ));
    let directory = tempdir().unwrap();
    let executable = directory.path().join(if cfg!(windows) {
        "hyphae.exe"
    } else {
        "hyphae"
    });
    std::fs::write(&executable, b"not the reviewed executable").unwrap();
    assert!(matches!(
        HyphaeInstallation::admit(&executable, &authority),
        Err(error) if error.code() == "executable_digest"
    ));
}

#[tokio::test]
async fn transport_rejects_non_loopback_endpoints() {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 8788);
    assert!(matches!(
        NativeHttpClient::new(address, Duration::from_secs(1)),
        Err(error) if error.code() == "non_loopback_endpoint"
    ));
}

#[tokio::test]
async fn value_and_idempotency_bounds_are_checked_before_io() {
    let client =
        NativeHttpClient::new("127.0.0.1:9".parse().unwrap(), Duration::from_secs(1)).unwrap();
    let value = vec![0; MAX_VALUE_BYTES + 1];
    assert!(matches!(
        client.set(b"key", &value, 1, 7).await,
        Err(error) if error.code() == "value_too_large"
    ));
    assert!(matches!(
        client.set(b"key", b"value", 0, 7).await,
        Err(error) if error.code() == "invalid_idempotency_token"
    ));
}

#[tokio::test]
async fn capabilities_mismatch_is_incompatible_capabilities() {
    let mut body = decode_hex(CAPABILITIES_RESPONSE);
    body[16..18].copy_from_slice(&2_u16.to_le_bytes());
    let (address, server) =
        raw_response_server(move || http_response(PRODUCT_MEDIA_TYPE, "7", &body, "200 OK")).await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.validate_capabilities(7).await,
        Err(error) if error.code() == "incompatible_capabilities"
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn transport_preserves_unknown_typed_errors() {
    let body = decode_hex(UNKNOWN_ERROR);
    let (address, server) = raw_response_server(move || {
        http_response(ERROR_MEDIA_TYPE, "7", &body, "500 Internal Server Error")
    })
    .await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.capabilities(7).await,
        Err(error) if error.product_code() == Some("future_failure")
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn transport_rejects_zero_or_mismatched_request_identity() {
    let body = decode_hex(CAPABILITIES_RESPONSE);
    let (address, server) =
        raw_response_server(move || http_response(PRODUCT_MEDIA_TYPE, "1", &body, "200 OK")).await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.capabilities(0).await,
        Err(error) if error.code() == "request_id"
    ));
    server.abort();

    let body = decode_hex(CAPABILITIES_RESPONSE);
    let (address, server) =
        raw_response_server(move || http_response(PRODUCT_MEDIA_TYPE, "8", &body, "200 OK")).await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.capabilities(7).await,
        Err(error) if error.code() == "request_id"
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn transport_rejects_wrong_media_type() {
    let body = decode_hex(CAPABILITIES_RESPONSE);
    let (address, server) = raw_response_server(move || {
        http_response("application/octet-stream", "7", &body, "200 OK")
    })
    .await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.capabilities(7).await,
        Err(error) if error.code() == "media_type"
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn transport_times_out_on_incomplete_body() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 2_048];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.hyphae.product-v1\r\nX-Hyphae-Request-Id: 7\r\nContent-Length: 72\r\n\r\nHYP")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
    });
    let client = NativeHttpClient::new(address, Duration::from_millis(50)).unwrap();
    assert!(matches!(
        client.capabilities(7).await,
        Err(error) if error.code() == "timeout"
    ));
    server.abort();
}

#[tokio::test]
async fn lost_ack_is_resolved_with_the_original_idempotency_token() {
    let transaction_id = 73_u128;
    let responses = vec![
        http_response(
            PRODUCT_MEDIA_TYPE,
            "8",
            &outcome_unknown_commit(transaction_id),
            "200 OK",
        ),
        http_response(
            PRODUCT_MEDIA_TYPE,
            "9",
            &committed_status(transaction_id, 0),
            "200 OK",
        ),
    ];
    let (address, server) = scripted_response_server(responses).await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();

    client.set(b"key", b"value", 97, 8).await.unwrap();

    let requests = server.await.unwrap();
    assert_eq!(product_operation(&requests[0]), 6);
    assert_eq!(product_operation(&requests[1]), 39);
    assert!(
        request_body(&requests[1])
            .windows(16)
            .any(|bytes| bytes == 97_u128.to_le_bytes())
    );
}

#[tokio::test]
async fn resolution_never_invents_success() {
    let transaction_id = 73_u128;
    for (status, expected_code) in [
        (unknown_status(), "outcome_unknown"),
        (rolled_back_status(transaction_id), "mutation_rolled_back"),
        (committed_status(transaction_id, 1), "non_strict_commit"),
        (committed_status(transaction_id + 1, 0), "outcome_unknown"),
    ] {
        let responses = vec![
            http_response(
                PRODUCT_MEDIA_TYPE,
                "8",
                &outcome_unknown_commit(transaction_id),
                "200 OK",
            ),
            http_response(PRODUCT_MEDIA_TYPE, "9", &status, "200 OK"),
        ];
        let (address, server) = scripted_response_server(responses).await;
        let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
        let error = client.set(b"key", b"value", 97, 8).await.unwrap_err();
        assert_eq!(error.code(), expected_code);
        server.await.unwrap();
    }
}

async fn raw_response_server(
    response: impl FnOnce() -> Vec<u8> + Send + 'static,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 4_096];
        let _ = socket.read(&mut request).await.unwrap();
        socket.write_all(&response()).await.unwrap();
    });
    (address, server)
}

async fn scripted_response_server(
    responses: Vec<Vec<u8>>,
) -> (SocketAddr, tokio::task::JoinHandle<Vec<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            requests.push(read_http_message(&mut socket).await);
            socket.write_all(&response).await.unwrap();
        }
        requests
    });
    (address, server)
}

async fn read_http_message(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut message = Vec::new();
    let mut chunk = [0_u8; 4_096];
    loop {
        let read = socket.read(&mut chunk).await.unwrap();
        assert_ne!(read, 0, "HTTP message ended before its declared body");
        message.extend_from_slice(&chunk[..read]);
        let Some(header_end) = message.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            continue;
        };
        let body_start = header_end + 4;
        let headers = std::str::from_utf8(&message[..header_end]).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        if message.len() >= body_start + content_length {
            return message;
        }
    }
}

fn http_response(content_type: &str, request_id: &str, body: &[u8], status: &str) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nX-Hyphae-Request-Id: {request_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| u8::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap())
        .collect()
}

fn outcome_unknown_commit(transaction_id: u128) -> Vec<u8> {
    product_response(5, {
        let mut payload = vec![1];
        payload.extend_from_slice(&[0; 7]);
        payload.extend_from_slice(&transaction_id.to_le_bytes());
        payload
    })
}

fn committed_status(transaction_id: u128, durability: u8) -> Vec<u8> {
    let mut payload = vec![1];
    payload.extend_from_slice(&transaction_id.to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&[0; 32]);
    payload.push(durability);
    payload.extend_from_slice(&[0; 7]);
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&0_u64.to_le_bytes());
    product_response(7, payload)
}

fn rolled_back_status(transaction_id: u128) -> Vec<u8> {
    let mut payload = vec![2];
    payload.extend_from_slice(&transaction_id.to_le_bytes());
    product_response(7, payload)
}

fn unknown_status() -> Vec<u8> {
    product_response(7, vec![0])
}

fn product_response(tag: u16, payload: Vec<u8>) -> Vec<u8> {
    let mut response = Vec::new();
    response.extend_from_slice(b"HYPRSP01");
    response.extend_from_slice(&u32::try_from(16 + payload.len()).unwrap().to_le_bytes());
    response.extend_from_slice(&tag.to_le_bytes());
    response.extend_from_slice(&0_u16.to_le_bytes());
    response.extend_from_slice(&payload);
    response
}

fn product_operation(request: &[u8]) -> u16 {
    let body = request_body(request);
    u16::from_le_bytes(body[12..14].try_into().unwrap())
}

fn request_body(request: &[u8]) -> &[u8] {
    let body_start = request
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    &request[body_start..]
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
