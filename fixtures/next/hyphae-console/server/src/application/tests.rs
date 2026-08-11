// SPDX-License-Identifier: AGPL-3.0-only

use super::*;
use http::header::{COOKIE, ORIGIN, SET_COOKIE};
use http_body_util::BodyExt;
use std::collections::BTreeMap;
use std::sync::{Mutex as StdMutex, MutexGuard};
use tower::ServiceExt;

use crate::{HyphaeInstallation, HyphaeSidecar, SidecarAuthority};

#[derive(Clone, Default)]
struct FakeStore {
    values: Arc<StdMutex<BTreeMap<String, Vec<u8>>>>,
    operations: Arc<StdMutex<Vec<(String, String)>>>,
}

impl ConsoleStateStore for FakeStore {
    fn get<'a>(
        &'a self,
        tenant_id: &'a str,
    ) -> state::StoreFuture<'a, Result<Option<Vec<u8>>, ConsoleStoreError>> {
        let values = self.values.clone();
        let operations = self.operations.clone();
        Box::pin(async move {
            lock(&operations).push(("get".to_owned(), tenant_id.to_owned()));
            Ok(lock(&values).get(tenant_id).cloned())
        })
    }

    fn put<'a>(
        &'a self,
        tenant_id: &'a str,
        value: &'a [u8],
        _idempotency_token: u128,
    ) -> state::StoreFuture<'a, Result<(), ConsoleStoreError>> {
        let values = self.values.clone();
        let operations = self.operations.clone();
        Box::pin(async move {
            lock(&operations).push(("put".to_owned(), tenant_id.to_owned()));
            lock(&values).insert(tenant_id.to_owned(), value.to_vec());
            Ok(())
        })
    }
}

#[tokio::test]
async fn login_rotates_session_and_scopes_console_to_the_server_tenant() {
    let store = FakeStore::default();
    let runtime = runtime(store.clone());
    let form = request(&runtime, "GET", "/login", None, None).await;
    assert_eq!(form.status(), StatusCode::OK);
    let anonymous = cookie(&form);
    let form_html = body_text(form).await;
    let csrf = hidden(&form_html, "_csrf");

    let login = request(
        &runtime,
        "POST",
        "/login",
        Some(&anonymous),
        Some(&format!(
            "username=alice&password=preview-only&_csrf={csrf}"
        )),
    )
    .await;
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    let authenticated = cookie(&login);
    assert_ne!(anonymous, authenticated);

    let denied = request(&runtime, "GET", "/console", Some(&anonymous), None).await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let console = request(&runtime, "GET", "/console", Some(&authenticated), None).await;
    assert_eq!(console.status(), StatusCode::OK);
    let html = body_text(console).await;
    assert!(html.contains("data-user=\"alice\""));
    assert!(html.contains("data-tenant=\"tenant-a\""));
    assert!(!html.contains("tenant-b"));
    assert_eq!(
        lock(&store.operations).as_slice(),
        &[("get".to_owned(), "tenant-a".to_owned())]
    );
}

#[tokio::test]
async fn tenant_mutations_are_isolated_and_mass_assignment_fails_before_storage() {
    let store = FakeStore::default();
    let runtime = runtime(store.clone());
    let alice = login(&runtime, "alice").await;
    let bob = login(&runtime, "bob").await;

    increment(&runtime, &alice, None).await;
    increment(&runtime, &bob, None).await;
    let puts_before = lock(&store.operations)
        .iter()
        .filter(|(operation, _)| operation == "put")
        .count();
    let rejected = increment(&runtime, &alice, Some("tenant=tenant-b")).await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        lock(&store.operations)
            .iter()
            .filter(|(operation, _)| operation == "put")
            .count(),
        puts_before
    );

    let alice_html =
        body_text(request(&runtime, "GET", "/console", Some(&alice), None).await).await;
    let bob_html = body_text(request(&runtime, "GET", "/console", Some(&bob), None).await).await;
    assert!(alice_html.contains("data-tenant=\"tenant-a\""));
    assert!(bob_html.contains("data-tenant=\"tenant-b\""));
    assert!(alice_html.contains("data-counter=\"1\""));
    assert!(bob_html.contains("data-counter=\"1\""));
    assert!(lock(&store.values).contains_key("tenant-a"));
    assert!(lock(&store.values).contains_key("tenant-b"));
}

#[tokio::test]
async fn csrf_failures_and_raw_sidecar_paths_never_reach_storage() {
    let store = FakeStore::default();
    let runtime = runtime(store.clone());
    let alice = login(&runtime, "alice").await;
    let console = request(&runtime, "GET", "/console", Some(&alice), None).await;
    let html = body_text(console).await;
    let revision = hidden(&html, "expected_revision");
    let before = lock(&store.operations).len();
    let response = request_with_origin(
        &runtime,
        "POST",
        "/console/increment",
        Some(&alice),
        Some(&format!("expected_revision={revision}&_csrf=invalid")),
        ORIGIN_VALUE,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(lock(&store.operations).len(), before);

    let raw = request(&runtime, "GET", "/v2/capabilities", Some(&alice), None).await;
    assert_eq!(raw.status(), StatusCode::NOT_FOUND);
    assert_eq!(lock(&store.operations).len(), before);
}

#[tokio::test]
async fn complete_and_ordered_pages_apply_the_no_exposure_policy() {
    let runtime = runtime(FakeStore::default());
    let alice = login(&runtime, "alice").await;
    let console = request(&runtime, "GET", "/console", Some(&alice), None).await;
    assert!(console.headers().contains_key(http::header::CONTENT_LENGTH));
    assert_eq!(console.headers()["cache-control"], "no-store");
    assert!(
        console.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("connect-src 'none'")
    );
    let console_html = body_text(console).await;
    assert!(!console_html.contains("<script"));
    assert!(!console_html.contains("/v2/"));
    assert!(!console_html.contains("application/vnd.hyphae"));

    let activity = request(&runtime, "GET", "/console/activity", Some(&alice), None).await;
    assert!(
        !activity
            .headers()
            .contains_key(http::header::CONTENT_LENGTH)
    );
    let activity_html = body_text(activity).await;
    assert!(activity_html.contains("Tenant activity"));
    assert!(activity_html.ends_with("</body></html>"));
}

#[tokio::test]
#[ignore = "requires HYPHAE_V101_BIN pointing to the reviewed release executable"]
async fn real_v101_console_preserves_tenant_state_across_full_restart() {
    let executable = std::env::var_os("HYPHAE_V101_BIN").expect("HYPHAE_V101_BIN");
    let authority = SidecarAuthority::load().unwrap();
    let installation =
        HyphaeInstallation::admit(std::path::Path::new(&executable), &authority).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("data");

    let mut sidecar = HyphaeSidecar::start(&installation, &data).await.unwrap();
    let runtime_a = real_runtime(sidecar.store());
    let old_alice = login(&runtime_a, "alice").await;
    assert_eq!(
        increment(&runtime_a, &old_alice, None).await.status(),
        StatusCode::SEE_OTHER
    );
    let bob = login(&runtime_a, "bob").await;
    assert!(
        body_text(request(&runtime_a, "GET", "/console", Some(&bob), None).await)
            .await
            .contains("data-counter=\"0\"")
    );
    sidecar.shutdown().unwrap();

    let mut sidecar = HyphaeSidecar::start(&installation, &data).await.unwrap();
    let runtime_b = real_runtime(sidecar.store());
    assert_eq!(
        request(&runtime_b, "GET", "/console", Some(&old_alice), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let new_alice = login(&runtime_b, "alice").await;
    let alice_html =
        body_text(request(&runtime_b, "GET", "/console", Some(&new_alice), None).await).await;
    assert!(alice_html.contains("data-counter=\"1\""));
    assert!(alice_html.contains("data-tenant=\"tenant-a\""));
    let new_bob = login(&runtime_b, "bob").await;
    let bob_html =
        body_text(request(&runtime_b, "GET", "/console", Some(&new_bob), None).await).await;
    assert!(bob_html.contains("data-counter=\"0\""));
    assert!(bob_html.contains("data-tenant=\"tenant-b\""));
    sidecar.shutdown().unwrap();
}

#[tokio::test]
#[ignore = "requires Node.js and a real Chrome acceptance lane"]
async fn real_browser_keeps_hyphae_outside_the_browser_boundary() {
    let store = FakeStore::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("http://localhost:{}", address.port());
    let runtime = browser_runtime(store, &origin);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        runtime
            .serve(listener, async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
        .join("scripts/check-next-hyphae-console-browser.mjs");
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .arg(script)
            .env("PLIEGO_HYPHAE_CONSOLE_URL", origin)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let _ = shutdown_tx.send(());
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "browser acceptance failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn runtime(store: FakeStore) -> NativeRuntime {
    runtime_for_origin(store, ORIGIN_VALUE)
}

fn runtime_for_origin(store: FakeStore, origin: &str) -> NativeRuntime {
    let mut key = vec![0_u8; 32];
    getrandom::fill(&mut key).unwrap();
    build_console_runtime_with_store(
        Arc::new(store),
        origin,
        SecretHandle::new("console-csrf", 1, key).unwrap(),
        None,
    )
    .unwrap()
}

fn browser_runtime(store: FakeStore, origin: &str) -> NativeRuntime {
    let mut key = vec![0_u8; 32];
    getrandom::fill(&mut key).unwrap();
    let cookie = pliego_runtime::SessionCookiePolicy::new("pliego-browser-session")
        .unwrap()
        .secure(false)
        .unwrap();
    build_console_runtime_with_store(
        Arc::new(store),
        origin,
        SecretHandle::new("console-csrf", 1, key).unwrap(),
        Some(cookie),
    )
    .unwrap()
}

fn real_runtime(store: ConsoleStore) -> NativeRuntime {
    let mut key = vec![0_u8; 32];
    getrandom::fill(&mut key).unwrap();
    build_console_runtime(
        store,
        ORIGIN_VALUE,
        SecretHandle::new("console-csrf", 1, key).unwrap(),
    )
    .unwrap()
}

const ORIGIN_VALUE: &str = "https://console.example.test";

async fn login(runtime: &NativeRuntime, username: &str) -> String {
    let form = request(runtime, "GET", "/login", None, None).await;
    let anonymous = cookie(&form);
    let csrf = hidden(&body_text(form).await, "_csrf");
    let response = request(
        runtime,
        "POST",
        "/login",
        Some(&anonymous),
        Some(&format!(
            "username={username}&password=preview-only&_csrf={csrf}"
        )),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    cookie(&response)
}

async fn increment(runtime: &NativeRuntime, cookie: &str, extra: Option<&str>) -> Response<Body> {
    let console = request(runtime, "GET", "/console", Some(cookie), None).await;
    let html = body_text(console).await;
    let csrf = hidden(&html, "_csrf");
    let revision = hidden(&html, "expected_revision");
    let mut body = format!("expected_revision={revision}&_csrf={csrf}");
    if let Some(extra) = extra {
        body.push('&');
        body.push_str(extra);
    }
    request(
        runtime,
        "POST",
        "/console/increment",
        Some(cookie),
        Some(&body),
    )
    .await
}

async fn request(
    runtime: &NativeRuntime,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<&str>,
) -> Response<Body> {
    request_with_origin(runtime, method, uri, cookie, body, ORIGIN_VALUE).await
}

async fn request_with_origin(
    runtime: &NativeRuntime,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<&str>,
    origin: &str,
) -> Response<Body> {
    let mut builder = pliego_runtime::Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(COOKIE, cookie);
    }
    if method == "POST" {
        builder = builder.header(ORIGIN, origin).header(
            http::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        );
    }
    runtime
        .router()
        .oneshot(
            builder
                .body(Body::from(body.unwrap_or_default().to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn cookie(response: &Response<Body>) -> String {
    response.headers()[SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

async fn body_text(response: Response<Body>) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

fn hidden(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let start = html.find(&marker).expect("hidden field exists") + marker.len();
    html[start..].split('"').next().unwrap().to_owned()
}

fn lock<T>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
