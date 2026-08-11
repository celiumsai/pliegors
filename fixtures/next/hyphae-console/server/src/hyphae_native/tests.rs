// SPDX-License-Identifier: AGPL-3.0-only

use super::wire::{ERROR_MEDIA_TYPE, PRODUCT_MEDIA_TYPE};
use super::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

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

#[test]
fn mutation_resolution_policy_distinguishes_uncertain_and_definitive_errors() {
    assert_eq!(mutation_resolution(&TransportError::Timeout), Some(None));
    assert_eq!(
        mutation_resolution(&TransportError::OutcomeUnknown(7)),
        Some(Some(7))
    );
    assert_eq!(mutation_resolution(&TransportError::NonStrictCommit), None);
    assert_eq!(
        mutation_resolution(&TransportError::NonLoopbackEndpoint(
            "192.0.2.1:8788".parse().unwrap()
        )),
        None
    );
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
        TransportError::Product { ref error, .. } if error.code == "future_failure"
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn product_unknown_commit_uses_matching_idempotency_resolution() {
    let transaction_id = 73_u128;
    let responses = vec![
        http_response(PRODUCT_MEDIA_TYPE, "1", &decode_hex(CAPABILITIES_RESPONSE)),
        error_response("3", &unknown_commit_error(3, transaction_id)),
        http_response(PRODUCT_MEDIA_TYPE, "4", &committed_status(transaction_id)),
    ];
    let (address, server) = scripted_response_server(responses).await;
    let store = ConsoleStore::new(NativeHttpClient::new(address, Duration::from_secs(1)).unwrap());

    store.put("tenant-a", b"resolved-value", 97).await.unwrap();

    let requests = join_with_timeout(server).await;
    assert_eq!(product_operation(&requests[1]), 6);
    assert_eq!(product_operation(&requests[2]), 39);
    assert!(
        request_body(&requests[2])
            .windows(16)
            .any(|bytes| bytes == 97_u128.to_le_bytes())
    );
}

#[tokio::test]
async fn product_unknown_commit_requires_v101_status_and_transaction_identity() {
    let transaction_id = 73_u128;
    let wrong_status = vec![
        http_response(PRODUCT_MEDIA_TYPE, "1", &decode_hex(CAPABILITIES_RESPONSE)),
        error_response_with_status(
            "500 Internal Server Error",
            "3",
            &unknown_commit_error(3, transaction_id),
        ),
    ];
    let (address, server) = scripted_response_server(wrong_status).await;
    let store = ConsoleStore::new(NativeHttpClient::new(address, Duration::from_secs(1)).unwrap());
    assert!(matches!(
        store.put("tenant-a", b"value", 97).await,
        Err(ConsoleStoreError::Transport(TransportError::Product { .. }))
    ));
    assert_eq!(join_with_timeout(server).await.len(), 2);

    let mismatched_transaction = vec![
        http_response(PRODUCT_MEDIA_TYPE, "1", &decode_hex(CAPABILITIES_RESPONSE)),
        error_response("3", &unknown_commit_error(3, transaction_id)),
        http_response(
            PRODUCT_MEDIA_TYPE,
            "4",
            &committed_status(transaction_id + 1),
        ),
    ];
    let (address, server) = scripted_response_server(mismatched_transaction).await;
    let store = ConsoleStore::new(NativeHttpClient::new(address, Duration::from_secs(1)).unwrap());
    assert!(matches!(
        store.put("tenant-a", b"value", 97).await,
        Err(ConsoleStoreError::OutcomeUnknown)
    ));
    assert_eq!(join_with_timeout(server).await.len(), 3);
}

#[tokio::test]
async fn zero_idempotency_token_is_rejected_before_transport() {
    let client =
        NativeHttpClient::new("127.0.0.1:9".parse().unwrap(), Duration::from_secs(1)).unwrap();
    let store = ConsoleStore::new(client);
    assert!(matches!(
        store.put("tenant-a", b"value", 0).await,
        Err(ConsoleStoreError::InvalidIdempotencyToken)
    ));
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
    store
        .put("tenant-a", b"persistent-value", 41)
        .await
        .unwrap();
    assert_eq!(
        store.get("tenant-a").await.unwrap(),
        Some(b"persistent-value".to_vec())
    );
    sidecar.shutdown().unwrap();

    let mut sidecar = HyphaeSidecar::start(&installation, &data).await.unwrap();
    let store = ConsoleStore::new(sidecar.client().clone());
    assert_eq!(
        store.get("tenant-a").await.unwrap(),
        Some(b"persistent-value".to_vec())
    );
    sidecar.shutdown().unwrap();
}

#[tokio::test]
#[ignore = "requires HYPHAE_V101_BIN pointing to the reviewed release executable"]
async fn real_v101_dropped_set_ack_resolves_committed_through_proxy() {
    let executable = std::env::var_os("HYPHAE_V101_BIN").expect("HYPHAE_V101_BIN");
    let authority = SidecarAuthority::load().unwrap();
    let installation = HyphaeInstallation::admit(Path::new(&executable), &authority).unwrap();
    let directory = tempdir().unwrap();
    let data = directory.path().join("data");
    let mut sidecar = HyphaeSidecar::start(&installation, &data).await.unwrap();
    let proxy = FaultProxy::start(sidecar.client().endpoint(), true).await;
    let store =
        ConsoleStore::new(NativeHttpClient::new(proxy.address, Duration::from_secs(5)).unwrap());

    store
        .put("tenant-a", b"committed-through-proxy", 101)
        .await
        .unwrap();
    assert_eq!(
        store.get("tenant-a").await.unwrap(),
        Some(b"committed-through-proxy".to_vec())
    );
    let observations = proxy.stop().await;
    assert_eq!(operations(&observations), vec![6, 39, 5]);
    sidecar.shutdown().unwrap();
}

#[tokio::test]
#[ignore = "requires HYPHAE_V101_BIN pointing to the reviewed release executable"]
async fn real_v101_fault_proxy_observes_unknown_then_recovered_rollback() {
    let executable = std::env::var_os("HYPHAE_V101_BIN").expect("HYPHAE_V101_BIN");
    let authority = SidecarAuthority::load().unwrap();
    let installation = HyphaeInstallation::admit(Path::new(&executable), &authority).unwrap();
    let directory = tempdir().unwrap();
    let data = directory.path().join("data");
    let token = 103;

    let mut sidecar = HyphaeSidecar::start(&installation, &data).await.unwrap();
    replace_blob_staging_directory_with_file(&data);
    let proxy = FaultProxy::start(sidecar.client().endpoint(), false).await;
    let store =
        ConsoleStore::new(NativeHttpClient::new(proxy.address, Duration::from_secs(5)).unwrap());
    let large_value = vec![7_u8; 9_000];
    assert!(matches!(
        store.put("tenant-a", &large_value, token).await,
        Err(ConsoleStoreError::OutcomeUnknown)
    ));
    let observations = proxy.stop().await;
    assert_eq!(operations(&observations), vec![6, 39]);
    let set_error = decode_error(response_body(&observations[0].response)).unwrap();
    assert_eq!(set_error.code, "unknown_commit");
    assert_eq!(
        set_error.transaction_state,
        TransactionState::OutcomeUnknown
    );
    assert!(matches!(
        decode_response(response_body(&observations[1].response)).unwrap(),
        ProductResponse::TransactionStatus(TransactionStatus::OutcomeUnknown(_))
    ));
    sidecar.shutdown().unwrap();

    restore_blob_staging_directory(&data);
    let mut sidecar = HyphaeSidecar::start(&installation, &data).await.unwrap();
    let proxy = FaultProxy::start(sidecar.client().endpoint(), false).await;
    let store =
        ConsoleStore::new(NativeHttpClient::new(proxy.address, Duration::from_secs(5)).unwrap());
    assert!(matches!(
        store.resolve_mutation(token, None).await,
        Err(ConsoleStoreError::MutationRolledBack)
    ));
    let observations = proxy.stop().await;
    assert_eq!(operations(&observations), vec![39]);
    assert!(matches!(
        decode_response(response_body(&observations[0].response)).unwrap(),
        ProductResponse::TransactionStatus(TransactionStatus::RolledBack(_))
    ));
    sidecar.shutdown().unwrap();
}

struct FaultProxy {
    address: SocketAddr,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Vec<ProxyObservation>>,
}

#[derive(Debug)]
struct ProxyObservation {
    operation: u16,
    response: Vec<u8>,
}

impl FaultProxy {
    async fn start(upstream: SocketAddr, drop_first_set_response: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(run_fault_proxy(
            listener,
            upstream,
            drop_first_set_response,
            shutdown_rx,
        ));
        Self {
            address,
            shutdown,
            task,
        }
    }

    async fn stop(self) -> Vec<ProxyObservation> {
        let _ = self.shutdown.send(());
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .expect("fault proxy shutdown timed out")
            .unwrap()
    }
}

async fn run_fault_proxy(
    listener: TcpListener,
    upstream: SocketAddr,
    drop_first_set_response: bool,
    mut shutdown: oneshot::Receiver<()>,
) -> Vec<ProxyObservation> {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let mut dropped_set = false;
    loop {
        let accepted = tokio::select! {
            biased;
            _ = &mut shutdown => break,
            accepted = listener.accept() => accepted,
        };
        let (mut downstream, _) = accepted.unwrap();
        let request = read_http_message(&mut downstream).await;
        let operation = product_operation_if_present(&request);
        let mut upstream_socket = TcpStream::connect(upstream).await.unwrap();
        upstream_socket.write_all(&request).await.unwrap();
        let response = read_http_message(&mut upstream_socket).await;
        if let Some(operation) = operation {
            operations.lock().unwrap().push(ProxyObservation {
                operation,
                response: response.clone(),
            });
        }
        if drop_first_set_response && operation == Some(6) && !dropped_set {
            assert!(matches!(
                decode_response(response_body(&response)).unwrap(),
                ProductResponse::Commit(wire::CommitOutcome::Committed(ref receipt))
                    if receipt.durability == 0
            ));
            dropped_set = true;
            continue;
        }
        downstream.write_all(&response).await.unwrap();
    }
    Arc::try_unwrap(operations).unwrap().into_inner().unwrap()
}

fn operations(observations: &[ProxyObservation]) -> Vec<u16> {
    observations
        .iter()
        .map(|observation| observation.operation)
        .collect()
}

fn replace_blob_staging_directory_with_file(data: &Path) {
    let blobs = data.join("tmp").join("blobs");
    if blobs.exists() {
        std::fs::remove_dir_all(&blobs).unwrap();
    }
    std::fs::write(blobs, b"fault").unwrap();
}

fn restore_blob_staging_directory(data: &Path) {
    let blobs = data.join("tmp").join("blobs");
    std::fs::remove_file(&blobs).unwrap();
    std::fs::create_dir_all(blobs).unwrap();
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

async fn join_with_timeout<T>(task: tokio::task::JoinHandle<T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("scripted server timed out")
        .unwrap()
}

async fn read_http_message(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut message = Vec::new();
    let mut chunk = [0_u8; 4_096];
    loop {
        let read = socket.read(&mut chunk).await.unwrap();
        assert_ne!(read, 0, "HTTP message ended before its declared body");
        message.extend_from_slice(&chunk[..read]);
        assert!(
            message.len() <= 17 * 1024 * 1024,
            "HTTP message exceeded test bound"
        );
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

fn http_response(content_type: &str, request_id: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nX-Hyphae-Request-Id: {request_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn error_response(request_id: &str, body: &[u8]) -> Vec<u8> {
    error_response_with_status("503 Service Unavailable", request_id, body)
}

fn error_response_with_status(status: &str, request_id: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ERROR_MEDIA_TYPE}\r\nX-Hyphae-Request-Id: {request_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn unknown_commit_error(request_id: u128, transaction_id: u128) -> Vec<u8> {
    let code = b"unknown_commit";
    let message = b"native transaction publication outcome is unknown";
    let mut body = Vec::new();
    body.extend_from_slice(b"HYPERR01");
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&[8, 5, 4, 1, u8::try_from(code.len()).unwrap()]);
    body.extend_from_slice(&u16::try_from(message.len()).unwrap().to_le_bytes());
    body.push(1);
    body.extend_from_slice(code);
    body.extend_from_slice(message);
    body.extend_from_slice(&request_id.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&16_u16.to_le_bytes());
    body.extend_from_slice(&transaction_id.to_le_bytes());
    let length = u32::try_from(body.len()).unwrap().to_le_bytes();
    body[8..12].copy_from_slice(&length);
    body
}

fn committed_status(transaction_id: u128) -> Vec<u8> {
    let mut payload = vec![1];
    payload.extend_from_slice(&transaction_id.to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&[0; 32]);
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.extend_from_slice(&0_u64.to_le_bytes());
    let mut response = Vec::new();
    response.extend_from_slice(b"HYPRSP01");
    response.extend_from_slice(&u32::try_from(16 + payload.len()).unwrap().to_le_bytes());
    response.extend_from_slice(&7_u16.to_le_bytes());
    response.extend_from_slice(&0_u16.to_le_bytes());
    response.extend_from_slice(&payload);
    response
}

fn product_operation(request: &[u8]) -> u16 {
    let body = request_body(request);
    u16::from_le_bytes(body[12..14].try_into().unwrap())
}

fn product_operation_if_present(request: &[u8]) -> Option<u16> {
    let body = request_body(request);
    body.starts_with(b"HYPREQ01")
        .then(|| u16::from_le_bytes(body[12..14].try_into().unwrap()))
}

fn request_body(request: &[u8]) -> &[u8] {
    let body_start = request
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    &request[body_start..]
}

fn response_body(response: &[u8]) -> &[u8] {
    request_body(response)
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
