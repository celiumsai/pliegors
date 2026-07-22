// SPDX-License-Identifier: Apache-2.0

use fullstack_pliego::build_cluster;
use http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, SET_COOKIE};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use pliego_runtime::{Body, DataOperation, DataOutcome, NativeRuntime, RuntimeReceipt};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tower::ServiceExt;

const EXPECTED_ORIGIN: &str = "https://example.com";

async fn dispatch(runtime: &NativeRuntime, request: Request<Body>) -> Response<Body> {
    runtime
        .router()
        .oneshot(request)
        .await
        .expect("runtime request")
}

async fn get(runtime: &NativeRuntime, target: &str, cookie: Option<&str>) -> Response<Body> {
    let mut builder = Request::builder().method("GET").uri(target);
    if let Some(cookie) = cookie {
        builder = builder.header(COOKIE, cookie);
    }
    dispatch(runtime, builder.body(Body::empty()).unwrap()).await
}

async fn post_form(
    runtime: &NativeRuntime,
    target: &str,
    cookie: &str,
    fields: &[(&str, &str)],
) -> Response<Body> {
    let fields = fields.iter().copied().collect::<BTreeMap<_, _>>();
    let body = serde_urlencoded::to_string(fields).unwrap();
    dispatch(
        runtime,
        Request::builder()
            .method("POST")
            .uri(target)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(ORIGIN, EXPECTED_ORIGIN)
            .header(COOKIE, cookie)
            .body(Body::from(body))
            .unwrap(),
    )
    .await
}

fn cookie_pair(response: &Response<Body>) -> String {
    response
        .headers()
        .get(SET_COOKIE)
        .expect("Set-Cookie")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

async fn body(response: Response<Body>) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .expect("response body")
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

fn hidden(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let tail = html
        .split_once(&marker)
        .unwrap_or_else(|| panic!("missing hidden input {name}"))
        .1;
    tail.split_once('"').unwrap().0.to_owned()
}

async fn login(
    form_runtime: &NativeRuntime,
    action_runtime: &NativeRuntime,
    user: &str,
) -> (String, String) {
    let form = get(form_runtime, "/login", None).await;
    assert_eq!(form.status(), StatusCode::OK);
    assert_eq!(form.headers()[CACHE_CONTROL], "no-store");
    let anonymous_cookie = cookie_pair(&form);
    let html = body(form).await;
    assert!(!html.contains("<script"));
    let csrf = hidden(&html, "_csrf");
    let result = post_form(
        action_runtime,
        "/login",
        &anonymous_cookie,
        &[
            ("username", user),
            ("password", "preview-only"),
            ("_csrf", &csrf),
        ],
    )
    .await;
    assert_eq!(result.status(), StatusCode::SEE_OTHER);
    assert_eq!(result.headers()[LOCATION], "/dashboard");
    let authenticated_cookie = cookie_pair(&result);
    assert_ne!(anonymous_cookie, authenticated_cookie);
    (anonymous_cookie, authenticated_cookie)
}

fn successful_action_receipt(receipts: &[RuntimeReceipt], deduplicated: bool) -> &RuntimeReceipt {
    receipts
        .iter()
        .rev()
        .find(|receipt| {
            receipt.route_id.as_deref() == Some("rename")
                && receipt.data_receipts.iter().any(|data| {
                    data.operation == DataOperation::Action
                        && data.operation_id == "rename-account"
                        && data.outcome == DataOutcome::Success
                        && data.deduplicated == deduplicated
                })
        })
        .expect("successful rename receipt")
}

#[tokio::test]
async fn fullstack_contract_holds_across_two_runtime_replicas() {
    let cluster = build_cluster().unwrap();
    assert_eq!(
        cluster.first.contract_sha256(),
        cluster.second.contract_sha256(),
        "replicas must execute one sealed application contract"
    );
    let (old_cookie, alice_cookie) = login(&cluster.first, &cluster.second, "alice").await;

    assert_eq!(
        get(&cluster.first, "/dashboard", Some(&old_cookie))
            .await
            .status(),
        StatusCode::UNAUTHORIZED,
        "rotation must revoke the pre-authentication token"
    );

    let first_dashboard = get(&cluster.first, "/dashboard", Some(&alice_cookie)).await;
    assert_eq!(first_dashboard.status(), StatusCode::OK);
    let first_dashboard = body(first_dashboard).await;
    assert!(first_dashboard.contains("Signed in as <strong>alice</strong>"));
    assert!(!first_dashboard.contains("<script"));
    let csrf = hidden(&first_dashboard, "_csrf");
    let idempotency_key = hidden(&first_dashboard, "idempotency_key");

    assert_eq!(
        get(&cluster.second, "/dashboard", Some(&alice_cookie))
            .await
            .status(),
        StatusCode::OK
    );
    let before_catalog_a = body(get(&cluster.first, "/catalog", None).await).await;
    let before_catalog_b = body(get(&cluster.second, "/catalog", None).await).await;
    assert!(before_catalog_a.contains("alice"));
    assert!(before_catalog_b.contains("alice"));

    let rename = [
        ("display_name", "Alice Prime"),
        ("idempotency_key", idempotency_key.as_str()),
        ("_csrf", csrf.as_str()),
    ];
    let committed = post_form(&cluster.first, "/account/rename", &alice_cookie, &rename).await;
    assert_eq!(committed.status(), StatusCode::SEE_OTHER);
    drop(committed);
    assert_eq!(cluster.mutation_count(), 1);

    let replayed = post_form(&cluster.second, "/account/rename", &alice_cookie, &rename).await;
    assert_eq!(replayed.status(), StatusCode::SEE_OTHER);
    drop(replayed);
    assert_eq!(cluster.mutation_count(), 1, "replay must not mutate twice");

    let conflict = post_form(
        &cluster.second,
        "/account/rename",
        &alice_cookie,
        &[
            ("display_name", "A different value"),
            ("idempotency_key", idempotency_key.as_str()),
            ("_csrf", csrf.as_str()),
        ],
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    drop(conflict);
    assert_eq!(cluster.mutation_count(), 1);

    let dashboard_after = body(get(&cluster.second, "/dashboard", Some(&alice_cookie)).await).await;
    assert!(dashboard_after.contains("Alice Prime"));
    let catalog_after = get(&cluster.second, "/catalog", None).await;
    assert_eq!(catalog_after.headers()[CACHE_CONTROL], "public, max-age=60");
    assert!(body(catalog_after).await.contains("Alice Prime"));

    let (_, bob_cookie) = login(&cluster.second, &cluster.first, "bob").await;
    let bob_dashboard = body(get(&cluster.first, "/dashboard", Some(&bob_cookie)).await).await;
    assert!(bob_dashboard.contains("Signed in as <strong>bob</strong>"));
    assert!(!bob_dashboard.contains("Alice Prime"));

    let tampered_csrf = format!("{csrf}x");
    let rejected = post_form(
        &cluster.second,
        "/account/rename",
        &alice_cookie,
        &[
            ("display_name", "Should not commit"),
            ("idempotency_key", "rename-request-0002"),
            ("_csrf", &tampered_csrf),
        ],
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    drop(rejected);
    assert_eq!(cluster.mutation_count(), 1);

    let first_receipts = cluster.first_receipts.receipts();
    let committed_receipt = successful_action_receipt(&first_receipts, false);
    assert_eq!(
        committed_receipt.application_contract_sha256,
        cluster.first.contract_sha256()
    );
    assert_eq!(committed_receipt.invalidation_events.len(), 2);
    assert!(
        committed_receipt
            .invalidation_events
            .iter()
            .all(|event| event.acknowledged())
    );
    let second_receipts = cluster.second_receipts.receipts();
    let replay_receipt = successful_action_receipt(&second_receipts, true);
    assert_eq!(
        replay_receipt.application_contract_sha256,
        cluster.second.contract_sha256()
    );
    assert!(replay_receipt.invalidation_events.is_empty());

    let evidence = serde_json::to_string(&(first_receipts, second_receipts)).unwrap();
    for secret in ["preview-only", csrf.as_str(), "Alice Prime", "alice"] {
        assert!(!evidence.contains(secret), "receipt leaked sensitive input");
    }

    assert!(cluster.revoke_cookie(&alice_cookie).await.unwrap());
    assert_eq!(
        get(&cluster.second, "/dashboard", Some(&alice_cookie))
            .await
            .status(),
        StatusCode::UNAUTHORIZED,
        "revocation must be visible to every replica"
    );
}

#[tokio::test]
async fn invalid_credentials_return_a_js_off_typed_form_error() {
    let cluster = build_cluster().unwrap();
    let form = get(&cluster.first, "/login", None).await;
    let cookie = cookie_pair(&form);
    let csrf = hidden(&body(form).await, "_csrf");
    let response = post_form(
        &cluster.second,
        "/login",
        &cookie,
        &[
            ("username", "alice"),
            ("password", "incorrect"),
            ("_csrf", &csrf),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body(response).await;
    assert!(html.contains("role=\"alert\""));
    assert!(html.contains("credentials"));
    assert!(!html.contains("<script"));
}

#[tokio::test]
async fn unknown_action_fields_fail_before_application_mutation() {
    let cluster = build_cluster().unwrap();
    let (_, cookie) = login(&cluster.first, &cluster.second, "alice").await;
    let dashboard = body(get(&cluster.first, "/dashboard", Some(&cookie)).await).await;
    let csrf = hidden(&dashboard, "_csrf");
    let key = hidden(&dashboard, "idempotency_key");
    let response = post_form(
        &cluster.second,
        "/account/rename",
        &cookie,
        &[
            ("display_name", "Alice Prime"),
            ("idempotency_key", &key),
            ("_csrf", &csrf),
            ("role", "admin"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    drop(response);
    assert_eq!(cluster.mutation_count(), 0);
}

#[tokio::test]
async fn typed_loader_failure_maps_to_an_authored_not_found_response() {
    let cluster = build_cluster().unwrap();
    let missing = get(&cluster.first, "/accounts/missing", None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let html = body(missing).await;
    assert!(html.contains("Account not found"));
    assert!(!html.contains("account-not-found"));
    assert!(!html.contains("<script"));

    let receipts = cluster.first_receipts.receipts();
    let receipt = receipts
        .iter()
        .find(|receipt| receipt.route_id.as_deref() == Some("account-profile"))
        .expect("account profile receipt");
    assert!(receipt.data_receipts.iter().any(|data| {
        data.operation == DataOperation::Loader
            && data.operation_id == "public-account-loader"
            && data.outcome == DataOutcome::Failed
    }));

    let (_, cookie) = login(&cluster.second, &cluster.first, "alice").await;
    let existing = get(&cluster.second, "/accounts/alice", Some(&cookie)).await;
    assert_eq!(existing.status(), StatusCode::OK);
    assert!(body(existing).await.contains("alice"));
}

async fn spawn_native(
    runtime: NativeRuntime,
) -> (
    std::net::SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<std::io::Result<()>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        runtime
            .serve(listener, async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    (address, shutdown_tx, task)
}

async fn socket_get(address: std::net::SocketAddr, target: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
        .await
        .expect("native response timeout")
        .unwrap();
    response
}

#[tokio::test]
async fn two_native_tcp_instances_serve_the_same_application_contract() {
    let cluster = build_cluster().unwrap();
    assert_eq!(
        cluster.first.contract_sha256(),
        cluster.second.contract_sha256()
    );
    let (first_address, first_shutdown, first_task) = spawn_native(cluster.first).await;
    let (second_address, second_shutdown, second_task) = spawn_native(cluster.second).await;

    let (first, second) = tokio::join!(
        socket_get(first_address, "/login"),
        socket_get(second_address, "/login")
    );
    for response in [first, second] {
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(
            response
                .windows(16)
                .any(|window| window == b"PliegoRS G2</h1>")
        );
    }

    first_shutdown.send(()).unwrap();
    second_shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), first_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), second_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
