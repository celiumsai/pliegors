// SPDX-License-Identifier: Apache-2.0

use http::HeaderMap;
use pliego_runtime::{
    InMemorySessionStore, SessionManager, SessionPolicy, expire_session_cookie_header,
    read_session_token, session_cookie_header,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Claims {
    subject: String,
}

#[tokio::test]
async fn secure_session_cookie_round_trips_without_exposing_server_claims() {
    let manager = SessionManager::new(
        SessionPolicy::new("application-session", 1).unwrap(),
        InMemorySessionStore::new(),
    );
    let created = manager
        .create(Claims {
            subject: "private-user".to_owned(),
        })
        .await
        .unwrap();
    let (_, set_cookie) = session_cookie_header(&created.cookie).unwrap();
    let set_cookie = set_cookie.to_str().unwrap();
    assert!(set_cookie.starts_with("__Host-pliego-session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("Path=/"));
    assert!(!set_cookie.contains("Domain="));
    assert!(!set_cookie.contains("private-user"));

    let pair = set_cookie.split(';').next().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("cookie", pair.parse().unwrap());
    let token = read_session_token(&headers, manager.policy().cookie_policy())
        .unwrap()
        .unwrap();
    assert_eq!(token, *created.cookie.token());
    assert_eq!(
        manager
            .load::<Claims>(&token)
            .await
            .unwrap()
            .unwrap()
            .claims
            .subject,
        "private-user"
    );

    let (_, expired) = expire_session_cookie_header(manager.policy().cookie_policy()).unwrap();
    assert!(expired.to_str().unwrap().contains("Max-Age=0"));
}

#[test]
fn duplicate_or_malformed_session_cookies_fail_closed() {
    let policy = SessionPolicy::new("application-session", 1).unwrap();
    let token = "A".repeat(43);
    let mut headers = HeaderMap::new();
    headers.insert(
        "cookie",
        format!("__Host-pliego-session={token}; __Host-pliego-session={token}")
            .parse()
            .unwrap(),
    );
    assert!(read_session_token(&headers, policy.cookie_policy()).is_err());

    headers.insert("cookie", "__Host-pliego-session=bad".parse().unwrap());
    assert!(read_session_token(&headers, policy.cookie_policy()).is_err());
}
