// SPDX-License-Identifier: Apache-2.0

use pliego_data::{CsrfManager, InMemorySessionStore, SecretHandle, SessionManager, SessionPolicy};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Claims {
    subject: String,
}

fn key(version: u32, byte: u8) -> SecretHandle {
    SecretHandle::new("csrf-key", version, vec![byte; 32]).unwrap()
}

#[tokio::test]
async fn csrf_token_is_bound_to_session_action_and_revision() {
    let sessions = SessionManager::new(
        SessionPolicy::new("application-session", 1).unwrap(),
        InMemorySessionStore::new(),
    );
    let first = sessions
        .create(Claims {
            subject: "user-1".to_owned(),
        })
        .await
        .unwrap();
    let second = sessions
        .create(Claims {
            subject: "user-2".to_owned(),
        })
        .await
        .unwrap();
    let manager = CsrfManager::new(key(1, 7), []).unwrap();
    let token = manager
        .issue(first.cookie.token(), "rename-account", 1)
        .unwrap();
    assert!(
        manager
            .verify(&token, first.cookie.token(), "rename-account", 1)
            .unwrap()
    );
    assert!(
        !manager
            .verify(&token, second.cookie.token(), "rename-account", 1)
            .unwrap()
    );
    assert!(
        !manager
            .verify(&token, first.cookie.token(), "delete-account", 1)
            .unwrap()
    );
    assert!(
        !manager
            .verify(&token, first.cookie.token(), "rename-account", 2)
            .unwrap()
    );
    assert_eq!(
        format!("{token:?}"),
        "CsrfToken { key_version: 1, mac: \"[REDACTED]\" }"
    );
    assert!(!format!("{token:?}").contains(&token.as_form_value()));
}

#[tokio::test]
async fn session_rotation_invalidates_old_binding_and_key_ring_reads_predecessor() {
    let sessions = SessionManager::new(
        SessionPolicy::new("application-session", 1).unwrap(),
        InMemorySessionStore::new(),
    );
    let created = sessions
        .create(Claims {
            subject: "user-1".to_owned(),
        })
        .await
        .unwrap();
    let old_manager = CsrfManager::new(key(1, 7), []).unwrap();
    let old_token = old_manager
        .issue(created.cookie.token(), "rename-account", 1)
        .unwrap();
    let rotated = sessions
        .rotate(
            created.cookie.token(),
            Claims {
                subject: "user-1".to_owned(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    let new_manager = CsrfManager::new(key(2, 9), [key(1, 7)]).unwrap();
    assert!(
        new_manager
            .verify(&old_token, created.cookie.token(), "rename-account", 1)
            .unwrap()
    );
    assert!(
        !new_manager
            .verify(&old_token, rotated.cookie.token(), "rename-account", 1)
            .unwrap()
    );
    let new_token = new_manager
        .issue(rotated.cookie.token(), "rename-account", 1)
        .unwrap();
    assert!(
        new_manager
            .verify(&new_token, rotated.cookie.token(), "rename-account", 1)
            .unwrap()
    );
}

#[test]
fn secret_debug_and_serializable_surfaces_do_not_expose_material() {
    let secret = key(1, 42);
    let output = format!("{secret:?}");
    assert!(output.contains("[REDACTED]"));
    assert!(!output.contains(&"42".repeat(32)));
    assert_eq!(secret.id(), "csrf-key");
    assert_eq!(secret.version(), 1);
}
