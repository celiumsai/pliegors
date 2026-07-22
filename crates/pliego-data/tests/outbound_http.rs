// SPDX-License-Identifier: Apache-2.0

use pliego_data::{
    DataContext, DataContextOptions, DataIdentity, DataRequestValues, OutboundDnsResolver,
    OutboundFuture, OutboundHttpError, OutboundHttpGuard, OutboundHttpPolicy, ResourceRegistry,
};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

#[derive(Clone)]
struct StaticResolver {
    addresses: Vec<SocketAddr>,
    delay: Duration,
}

impl OutboundDnsResolver for StaticResolver {
    fn resolve(
        &self,
        _host: String,
        _port: u16,
    ) -> OutboundFuture<Result<Vec<SocketAddr>, OutboundHttpError>> {
        let addresses = self.addresses.clone();
        let delay = self.delay;
        Box::pin(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            Ok(addresses)
        })
    }
}

fn context() -> (DataContext, pliego_data::DataContextControl) {
    DataContext::open(
        DataIdentity::new("request-http", "outbound", "deployment-1").unwrap(),
        Instant::now() + Duration::from_secs(2),
        ResourceRegistry::empty(),
        [],
        DataRequestValues::default(),
        DataContextOptions::default(),
    )
    .unwrap()
}

fn guard(addresses: &[&str]) -> OutboundHttpGuard<StaticResolver> {
    let policy = OutboundHttpPolicy::new("catalog-http", 1)
        .unwrap()
        .allow_host("example.com")
        .unwrap();
    OutboundHttpGuard::new(
        policy,
        StaticResolver {
            addresses: addresses
                .iter()
                .map(|address| address.parse().unwrap())
                .collect(),
            delay: Duration::ZERO,
        },
    )
    .unwrap()
}

#[tokio::test]
async fn permit_pins_only_public_allowlisted_addresses_and_redacts_url() {
    let (context, control) = context();
    let permit = guard(&["93.184.216.34:443"])
        .authorize(&context, "https://example.com/private?token=secret")
        .await
        .unwrap();
    assert_eq!(permit.host(), "example.com");
    assert_eq!(permit.resolved_addresses().len(), 1);
    assert_eq!(permit.max_response_bytes(), 8 * 1_024 * 1_024);
    let debug = format!("{permit:?}");
    assert!(!debug.contains("private"));
    assert!(!debug.contains("secret"));
    let receipts = context.receipts();
    assert_eq!(receipts.last().unwrap().operation_id, "catalog-http");
    let serialized = serde_json::to_string(&receipts).unwrap();
    assert!(!serialized.contains("secret"));
    control.close();
}

#[tokio::test]
async fn ssrf_corpus_rejects_private_reserved_and_mixed_dns_results() {
    for address in [
        "127.0.0.1:443",
        "10.0.0.1:443",
        "169.254.169.254:443",
        "100.64.0.1:443",
        "192.0.2.1:443",
        "198.18.0.1:443",
        "203.0.113.1:443",
        "[::1]:443",
        "[fc00::1]:443",
        "[fe80::1]:443",
        "[2001:db8::1]:443",
        "[::ffff:127.0.0.1]:443",
    ] {
        let (context, control) = context();
        assert_eq!(
            guard(&[address])
                .authorize(&context, "https://example.com/")
                .await,
            Err(OutboundHttpError::PrivateAddressRejected),
            "accepted {address}"
        );
        control.close();
    }
    let (context, control) = context();
    assert_eq!(
        guard(&["93.184.216.34:443", "127.0.0.1:443"])
            .authorize(&context, "https://example.com/")
            .await,
        Err(OutboundHttpError::PrivateAddressRejected)
    );
    control.close();
}

#[tokio::test]
async fn url_and_redirect_policy_fail_closed() {
    let (context, control) = context();
    let guard = guard(&["93.184.216.34:443"]);
    for (url, expected) in [
        ("http://example.com/", OutboundHttpError::SchemeRejected),
        ("https://other.example/", OutboundHttpError::HostRejected),
        ("https://example.com:8443/", OutboundHttpError::PortRejected),
        (
            "https://user:pass@example.com/",
            OutboundHttpError::ForbiddenUrlComponent,
        ),
        (
            "https://example.com/#secret",
            OutboundHttpError::ForbiddenUrlComponent,
        ),
    ] {
        assert_eq!(guard.authorize(&context, url).await, Err(expected));
    }

    let restricted = OutboundHttpPolicy::new("restricted-http", 1)
        .unwrap()
        .allow_host("example.com")
        .unwrap()
        .restrict_path_prefix("/v1/")
        .unwrap();
    let restricted = OutboundHttpGuard::new(
        restricted,
        StaticResolver {
            addresses: vec!["93.184.216.34:443".parse().unwrap()],
            delay: Duration::ZERO,
        },
    )
    .unwrap();
    for allowed in ["https://example.com/v1", "https://example.com/v1/items"] {
        restricted.authorize(&context, allowed).await.unwrap();
    }
    for rejected in [
        "https://example.com/v10/items",
        "https://example.com/admin",
        "https://example.com/v1/%2e%2e/admin",
    ] {
        assert_eq!(
            restricted.authorize(&context, rejected).await,
            Err(OutboundHttpError::PathRejected),
            "accepted {rejected}"
        );
    }
    assert!(
        OutboundHttpPolicy::new("invalid-path-http", 1)
            .unwrap()
            .restrict_path_prefix("/v1")
            .is_err()
    );

    let one_redirect = OutboundHttpPolicy::new("catalog-http", 1)
        .unwrap()
        .allow_host("example.com")
        .unwrap()
        .limits(Duration::from_secs(1), 1, 1_024, 4)
        .unwrap();
    let guard = OutboundHttpGuard::new(
        one_redirect,
        StaticResolver {
            addresses: vec!["93.184.216.34:443".parse().unwrap()],
            delay: Duration::ZERO,
        },
    )
    .unwrap();
    let first = guard
        .authorize(&context, "https://example.com/start")
        .await
        .unwrap();
    let second = guard
        .authorize_redirect(&context, &first, "/next")
        .await
        .unwrap();
    assert_eq!(second.redirects_remaining(), 0);
    assert_eq!(
        guard.authorize_redirect(&context, &second, "/again").await,
        Err(OutboundHttpError::RedirectLimit)
    );
    control.close();
}

#[tokio::test]
async fn dns_timeout_and_request_cancellation_are_contagious() {
    let policy = OutboundHttpPolicy::new("catalog-http", 1)
        .unwrap()
        .allow_host("example.com")
        .unwrap()
        .limits(Duration::from_millis(5), 0, 1_024, 4)
        .unwrap();
    let guard = OutboundHttpGuard::new(
        policy,
        StaticResolver {
            addresses: vec!["93.184.216.34:443".parse().unwrap()],
            delay: Duration::from_millis(50),
        },
    )
    .unwrap();
    let (context, control) = context();
    assert_eq!(
        guard.authorize(&context, "https://example.com/").await,
        Err(OutboundHttpError::Deadline)
    );
    control.cancel(pliego_data::DataCancelReason::ApplicationAbort);
    assert_eq!(
        guard.authorize(&context, "https://example.com/").await,
        Err(OutboundHttpError::Cancelled)
    );
    control.close();
}
