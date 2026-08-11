// SPDX-License-Identifier: GPL-3.0-only

use super::wire::{ERROR_MEDIA_TYPE, PRODUCT_MEDIA_TYPE};
use super::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
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
fn decodes_released_capability_and_value_vectors() {
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
fn preserves_bounded_unknown_product_errors() {
    let error = decode_error(&decode_hex(UNKNOWN_ERROR)).unwrap();
    assert_eq!(error.code, "future_failure");
    assert_eq!(error.category, ErrorCategory::Internal);
    assert_eq!(error.retry, RetryClass::AfterRecovery);
    assert_eq!(
        error.unknown_details,
        vec![UnknownDetail {
            tag: 42,
            value: vec![1, 2, 3]
        }]
    );
}

#[test]
fn authority_selects_only_reviewed_platform_artifacts() {
    let authority = SidecarAuthority::load().unwrap();
    assert_eq!(authority.release_tag(), "v1.0.1");
    assert_eq!(
        authority.release_revision(),
        "84161cf067141b60f4847b965ef77c5b749749c0"
    );
    assert!(authority.current_artifact().is_ok());
}

#[test]
fn installation_rejects_changed_executable_bytes() {
    let directory = tempdir().unwrap();
    let executable = directory.path().join(if cfg!(windows) {
        "hyphae.exe"
    } else {
        "hyphae"
    });
    std::fs::write(&executable, b"not the reviewed executable").unwrap();
    let authority = SidecarAuthority::load().unwrap();
    assert!(matches!(
        HyphaeInstallation::admit(&executable, &authority),
        Err(AdmissionError::ExecutableDigest { .. })
    ));
}

#[tokio::test]
async fn transport_rejects_non_loopback_endpoints() {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 8788);
    assert!(matches!(
        NativeHttpClient::new(address, Duration::from_secs(1)),
        Err(TransportError::NonLoopbackEndpoint(_))
    ));
}

#[tokio::test]
async fn transport_rejects_zero_request_identity() {
    let body = decode_hex(CAPABILITIES_RESPONSE);
    let (address, server) =
        raw_response_server(move |_| http_response(PRODUCT_MEDIA_TYPE, "1", &body)).await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.capabilities(0).await,
        Err(TransportError::RequestId)
    ));
    server.abort();
}

#[tokio::test]
async fn transport_rejects_wrong_media_type_and_request_identity() {
    let body = decode_hex(CAPABILITIES_RESPONSE);
    let (address, server) =
        raw_response_server(move |_| http_response("application/octet-stream", "7", &body)).await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.capabilities(7).await,
        Err(TransportError::MediaType(_))
    ));
    server.await.unwrap();

    let body = decode_hex(CAPABILITIES_RESPONSE);
    let (address, server) =
        raw_response_server(move |_| http_response(PRODUCT_MEDIA_TYPE, "8", &body)).await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.capabilities(7).await,
        Err(TransportError::RequestId)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn transport_times_out_on_an_incomplete_body() {
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
    assert_eq!(client.capabilities(7).await, Err(TransportError::Timeout));
    server.abort();
}

#[tokio::test]
async fn transport_preserves_unknown_typed_errors() {
    let body = decode_hex(UNKNOWN_ERROR);
    let (address, server) = raw_response_server(move |_| {
        let mut response = format!(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: {ERROR_MEDIA_TYPE}\r\nX-Hyphae-Request-Id: 7\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        )
        .into_bytes();
        response.extend_from_slice(&body);
        response
    })
    .await;
    let client = NativeHttpClient::new(address, Duration::from_secs(1)).unwrap();
    let error = client.capabilities(7).await.unwrap_err();
    assert!(matches!(
        error,
        TransportError::Product(ref product) if product.code == "future_failure"
    ));
    server.await.unwrap();
}

#[tokio::test]
#[ignore = "requires HYPHAE_V101_BIN pointing to the reviewed release executable"]
async fn real_v101_sidecar_persists_strict_state_across_restart() {
    let executable = std::env::var_os("HYPHAE_V101_BIN").expect("HYPHAE_V101_BIN");
    let authority = SidecarAuthority::load().unwrap();
    let installation = HyphaeInstallation::admit(Path::new(&executable), &authority).unwrap();
    installation.verify_version().unwrap();
    let directory = tempdir().unwrap();
    let data = directory.path().join("data");

    let mut sidecar = HyphaeSidecar::start(&installation, &data).await.unwrap();
    let store = ConsoleStore::new(sidecar.client().clone());
    store.put(b"persistent-value", 41).await.unwrap();
    assert_eq!(
        store.get().await.unwrap(),
        Some(b"persistent-value".to_vec())
    );
    sidecar.shutdown().unwrap();

    let mut sidecar = HyphaeSidecar::start(&installation, &data).await.unwrap();
    let store = ConsoleStore::new(sidecar.client().clone());
    assert_eq!(
        store.get().await.unwrap(),
        Some(b"persistent-value".to_vec())
    );
    sidecar.shutdown().unwrap();
}

async fn raw_response_server(
    response: impl Fn(&[u8]) -> Vec<u8> + Send + 'static,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 4_096];
        let read = socket.read(&mut request).await.unwrap();
        socket.write_all(&response(&request[..read])).await.unwrap();
    });
    (address, server)
}

fn http_response(content_type: &str, request_id: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nX-Hyphae-Request-Id: {request_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
