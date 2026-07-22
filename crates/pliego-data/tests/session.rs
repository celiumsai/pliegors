// SPDX-License-Identifier: Apache-2.0

use pliego_data::{
    InMemorySessionStore, SameSitePolicy, SessionCookiePolicy, SessionError, SessionManager,
    SessionPolicy, SessionToken,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Claims {
    subject: String,
    role: String,
}

#[tokio::test]
async fn create_load_rotate_and_revoke_prevent_session_fixation() {
    let store = InMemorySessionStore::new();
    let manager = SessionManager::new(
        SessionPolicy::new("application-session", 1).unwrap(),
        store.clone(),
    );
    let attacker = SessionToken::parse(&"A".repeat(43)).unwrap();
    assert!(manager.load::<Claims>(&attacker).await.unwrap().is_none());

    let created = manager
        .create(Claims {
            subject: "user-1".to_owned(),
            role: "member".to_owned(),
        })
        .await
        .unwrap();
    assert_ne!(created.cookie.token(), &attacker);
    assert_eq!(store.len(), 1);
    assert_eq!(
        manager
            .load::<Claims>(created.cookie.token())
            .await
            .unwrap()
            .unwrap()
            .claims
            .subject,
        "user-1"
    );

    let rotated = manager
        .rotate(
            created.cookie.token(),
            Claims {
                subject: "user-1".to_owned(),
                role: "admin".to_owned(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_ne!(rotated.cookie.token(), created.cookie.token());
    assert!(
        manager
            .load::<Claims>(created.cookie.token())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        manager
            .load::<Claims>(rotated.cookie.token())
            .await
            .unwrap()
            .unwrap()
            .claims
            .role,
        "admin"
    );
    assert!(manager.revoke(rotated.cookie.token()).await.unwrap());
    assert!(store.is_empty());
}

#[tokio::test]
async fn idle_expiry_removes_the_server_side_session() {
    let store = InMemorySessionStore::new();
    let policy = SessionPolicy::new("short-session", 1)
        .unwrap()
        .timeouts(Duration::from_millis(1), Duration::from_millis(50))
        .unwrap();
    let manager = SessionManager::new(policy, store.clone());
    let created = manager
        .create(Claims {
            subject: "user-1".to_owned(),
            role: "member".to_owned(),
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert!(
        manager
            .load::<Claims>(created.cookie.token())
            .await
            .unwrap()
            .is_none()
    );
    assert!(store.is_empty());
}

#[test]
fn cookie_policy_fails_closed_and_token_debug_is_redacted() {
    assert!(SessionCookiePolicy::default().is_secure());
    assert!(SessionCookiePolicy::default().is_http_only());
    assert_eq!(
        SessionCookiePolicy::default().same_site_value(),
        SameSitePolicy::Lax
    );
    assert!(
        SessionCookiePolicy::default()
            .clone()
            .secure(false)
            .is_err()
    );
    assert!(
        SessionCookiePolicy::new("pliego-session")
            .unwrap()
            .secure(false)
            .unwrap()
            .same_site(SameSitePolicy::None)
            .is_err()
    );
    assert!(SessionCookiePolicy::default().path("/app").is_err());
    assert!(
        SessionCookiePolicy::new("__Secure-pliego-session")
            .unwrap()
            .secure(false)
            .is_err()
    );
    assert!(
        SessionCookiePolicy::new("pliego-session")
            .unwrap()
            .same_site(SameSitePolicy::None)
            .unwrap()
            .secure(false)
            .is_err()
    );
    let token = SessionToken::parse(&"A".repeat(43)).unwrap();
    let output = format!("{token:?}");
    assert_eq!(output, "SessionToken([REDACTED])");
    assert!(!output.contains(token.as_cookie_value()));
}

#[tokio::test]
async fn claim_bounds_and_schema_versions_are_enforced() {
    let store = InMemorySessionStore::new();
    let manager = SessionManager::new(
        SessionPolicy::new("bounded-session", 1)
            .unwrap()
            .max_claim_bytes(64)
            .unwrap(),
        store.clone(),
    );
    let error = manager
        .create(Claims {
            subject: "x".repeat(100),
            role: "member".to_owned(),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, SessionError::ClaimsTooLarge { .. }));

    let created = manager
        .create(Claims {
            subject: "user-1".to_owned(),
            role: "member".to_owned(),
        })
        .await
        .unwrap();
    let incompatible =
        SessionManager::new(SessionPolicy::new("bounded-session", 2).unwrap(), store);
    assert_eq!(
        incompatible.load::<Claims>(created.cookie.token()).await,
        Err(SessionError::VersionMismatch)
    );
}

#[tokio::test]
async fn revocation_and_rotation_are_visible_through_independent_managers() {
    let store = InMemorySessionStore::new();
    let policy = SessionPolicy::new("distributed-session", 1).unwrap();
    let first = SessionManager::new(policy.clone(), store.clone());
    let second = SessionManager::new(policy, store);
    let created = first
        .create(Claims {
            subject: "user-1".to_owned(),
            role: "member".to_owned(),
        })
        .await
        .unwrap();
    assert!(
        second
            .load::<Claims>(created.cookie.token())
            .await
            .unwrap()
            .is_some()
    );
    let rotated = second
        .rotate(
            created.cookie.token(),
            Claims {
                subject: "user-1".to_owned(),
                role: "admin".to_owned(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(
        first
            .load::<Claims>(created.cookie.token())
            .await
            .unwrap()
            .is_none()
    );
    assert!(first.revoke(rotated.cookie.token()).await.unwrap());
    assert!(
        second
            .load::<Claims>(rotated.cookie.token())
            .await
            .unwrap()
            .is_none()
    );
}
