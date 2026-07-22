// SPDX-License-Identifier: Apache-2.0

use pliego_data::{
    IdempotencyDecision, IdempotencyError, IdempotencyKey, IdempotencyManager,
    IdempotencyPartition, IdempotencyPolicy, InMemoryIdempotencyStore,
};

fn key() -> IdempotencyKey {
    IdempotencyKey::parse("request-key-00000001").unwrap()
}

fn partition() -> IdempotencyPartition {
    IdempotencyPartition::from_identity("user-1").unwrap()
}

#[tokio::test]
async fn completed_result_replays_without_reacquiring_execution() {
    let store = InMemoryIdempotencyStore::new();
    let manager = IdempotencyManager::new(
        IdempotencyPolicy::new("standard-retry").unwrap(),
        store.clone(),
    );
    let decision = manager
        .begin(
            "rename-account",
            1,
            1,
            &key(),
            &partition(),
            &"a".repeat(64),
        )
        .await
        .unwrap();
    let IdempotencyDecision::Execute(permit) = decision else {
        panic!("first invocation must execute");
    };
    permit.complete(br#"{"ok":true}"#.to_vec()).await.unwrap();
    assert_eq!(store.len(), 1);
    let replay = manager
        .begin(
            "rename-account",
            1,
            1,
            &key(),
            &partition(),
            &"a".repeat(64),
        )
        .await
        .unwrap();
    assert_eq!(
        replay,
        IdempotencyDecision::Replay(br#"{"ok":true}"#.to_vec())
    );
}

#[tokio::test]
async fn in_progress_conflicting_and_unknown_retries_fail_closed() {
    let manager = IdempotencyManager::new(
        IdempotencyPolicy::new("standard-retry").unwrap(),
        InMemoryIdempotencyStore::new(),
    );
    let first = manager
        .begin(
            "rename-account",
            1,
            1,
            &key(),
            &partition(),
            &"a".repeat(64),
        )
        .await
        .unwrap();
    assert_eq!(
        manager
            .begin(
                "rename-account",
                1,
                1,
                &key(),
                &partition(),
                &"a".repeat(64),
            )
            .await,
        Err(IdempotencyError::InProgress)
    );
    assert_eq!(
        manager
            .begin(
                "rename-account",
                1,
                1,
                &key(),
                &partition(),
                &"b".repeat(64),
            )
            .await,
        Err(IdempotencyError::InputConflict)
    );
    let IdempotencyDecision::Execute(permit) = first else {
        panic!("first invocation must execute");
    };
    permit.mark_unknown().await.unwrap();
    assert_eq!(
        manager
            .begin(
                "rename-account",
                1,
                1,
                &key(),
                &partition(),
                &"a".repeat(64),
            )
            .await,
        Err(IdempotencyError::OutcomeUnknown)
    );
}

#[test]
fn key_and_partition_debug_output_is_redacted() {
    let key = key();
    let partition = partition();
    assert_eq!(format!("{key:?}"), "IdempotencyKey([REDACTED])");
    assert_eq!(format!("{partition:?}"), "IdempotencyPartition([REDACTED])");
}
