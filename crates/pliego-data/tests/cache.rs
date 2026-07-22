// SPDX-License-Identifier: Apache-2.0

use pliego_data::{
    CacheDomain, CacheError, CacheKeyInput, CacheManager, CacheOutcome, CachePartition,
    CachePolicy, CacheTag, DataCancelReason, DataContext, DataContextOptions, DataIdentity,
    DataRequestValues, InMemoryCacheStore, InMemoryInvalidationCoordinator, ResourceRegistry,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct CachedValue {
    value: String,
}

fn public_policy() -> CachePolicy {
    CachePolicy::new("catalog-cache", 1, "catalog", 1, CacheDomain::PublicRuntime)
        .unwrap()
        .vary("locale")
        .unwrap()
}

fn input() -> CacheKeyInput {
    CacheKeyInput::new("catalog-loader", 1, "a".repeat(64))
        .unwrap()
        .vary("locale", "en-US")
        .unwrap()
}

fn request_context(id: &str) -> (DataContext, pliego_data::DataContextControl) {
    DataContext::open(
        DataIdentity::new(id, "catalog", "deployment-1").unwrap(),
        Instant::now() + Duration::from_secs(2),
        ResourceRegistry::empty(),
        [],
        DataRequestValues::default(),
        DataContextOptions::default(),
    )
    .unwrap()
}

#[test]
fn key_policy_requires_exact_vary_and_private_partition() {
    let public = public_policy();
    assert!(matches!(
        public.key(CacheKeyInput::new("catalog-loader", 1, "a".repeat(64)).unwrap()),
        Err(CacheError::MissingVary)
    ));
    assert!(matches!(
        public.key(input().partition(CachePartition::from_identity("user-1").unwrap())),
        Err(CacheError::PartitionMismatch)
    ));

    let private = CachePolicy::new(
        "account-cache",
        1,
        "accounts",
        1,
        CacheDomain::PrivateSession,
    )
    .unwrap();
    let base = CacheKeyInput::new("account-loader", 1, "b".repeat(64)).unwrap();
    assert!(matches!(
        private.key(base.clone()),
        Err(CacheError::PartitionMismatch)
    ));
    let first = private
        .key(
            base.clone()
                .partition(CachePartition::from_identity("user-1").unwrap()),
        )
        .unwrap();
    let second = private
        .key(base.partition(CachePartition::from_identity("user-2").unwrap()))
        .unwrap();
    assert_ne!(first.digest(), second.digest());

    let request_private = CachePolicy::new(
        "request-cache",
        1,
        "request-values",
        1,
        CacheDomain::PrivateRequest,
    )
    .unwrap();
    let request_input = CacheKeyInput::new("request-loader", 1, "c".repeat(64)).unwrap();
    assert!(matches!(
        request_private.key(
            request_input
                .clone()
                .partition(CachePartition::from_identity("request-1").unwrap())
        ),
        Err(CacheError::PartitionMismatch)
    ));
    request_private
        .key(request_input.partition(CachePartition::from_request("request-1").unwrap()))
        .unwrap();
    assert_eq!(
        format!(
            "{:?}",
            CachePartition::from_identity("private-user").unwrap()
        ),
        "CachePartition([REDACTED])"
    );
}

#[test]
fn tenant_and_identity_partition_is_structured_and_unambiguous() {
    let first = CachePartition::from_tenant_and_identity("tenant-a", "user-1").unwrap();
    let second = CachePartition::from_tenant_and_identity("tenant-b", "user-1").unwrap();
    let ambiguous_left = CachePartition::from_tenant_and_identity("ab", "c").unwrap();
    let ambiguous_right = CachePartition::from_tenant_and_identity("a", "bc").unwrap();
    assert_ne!(first, second);
    assert_ne!(ambiguous_left, ambiguous_right);
    assert!(CachePartition::from_tenant_and_identity("", "user-1").is_err());
    assert!(CachePartition::from_tenant_and_identity("tenant-a", "user\n1").is_err());
    let debug = format!("{first:?}");
    assert_eq!(debug, "CachePartition([REDACTED])");
    assert!(!debug.contains("tenant-a"));
    assert!(!debug.contains("user-1"));
}

#[tokio::test]
async fn private_cache_never_crosses_identity_partitions() {
    let policy = CachePolicy::new(
        "account-cache",
        1,
        "accounts",
        1,
        CacheDomain::PrivateSession,
    )
    .unwrap();
    let manager = CacheManager::new(policy.clone(), InMemoryCacheStore::new(32).unwrap());
    let base = CacheKeyInput::new("account-loader", 1, "b".repeat(64)).unwrap();
    let first = policy
        .key(
            base.clone()
                .partition(CachePartition::from_identity("user-1").unwrap()),
        )
        .unwrap();
    let second = policy
        .key(base.partition(CachePartition::from_identity("user-2").unwrap()))
        .unwrap();
    manager
        .insert(
            &first,
            &CachedValue {
                value: "private-user-1".to_owned(),
            },
            [],
        )
        .await
        .unwrap();
    let first_lookup = manager.lookup::<CachedValue>(&first).await.unwrap();
    assert_eq!(first_lookup.receipt.outcome, CacheOutcome::Private);
    assert_eq!(first_lookup.value.unwrap().value, "private-user-1");
    let second_lookup = manager.lookup::<CachedValue>(&second).await.unwrap();
    assert_eq!(second_lookup.receipt.outcome, CacheOutcome::Miss);
    assert!(second_lookup.value.is_none());
    let evidence = format!("{:?}", first_lookup.receipt);
    assert!(!evidence.contains("private-user-1"));
    assert!(!evidence.contains("user-1"));
}

#[tokio::test]
async fn freshness_stale_and_expiry_are_explicit() {
    let policy = public_policy()
        .freshness(Duration::from_millis(1), Duration::from_millis(20))
        .unwrap();
    let manager = CacheManager::new(policy.clone(), InMemoryCacheStore::new(8).unwrap());
    let key = policy.key(input()).unwrap();
    manager
        .insert(
            &key,
            &CachedValue {
                value: "cached".to_owned(),
            },
            [],
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(
        manager
            .lookup::<CachedValue>(&key)
            .await
            .unwrap()
            .receipt
            .outcome,
        CacheOutcome::Stale
    );
    tokio::time::sleep(Duration::from_millis(25)).await;
    let expired = manager.lookup::<CachedValue>(&key).await.unwrap();
    assert_eq!(expired.receipt.outcome, CacheOutcome::Miss);
    assert!(expired.value.is_none());
}

#[tokio::test]
async fn causal_invalidation_acknowledges_two_registered_replicas() {
    let policy = public_policy();
    let first_store = InMemoryCacheStore::new(32).unwrap();
    let second_store = InMemoryCacheStore::new(32).unwrap();
    let coordinator = InMemoryInvalidationCoordinator::new();
    coordinator.register(&first_store);
    coordinator.register(&second_store);
    let first = CacheManager::new(policy.clone(), first_store.clone());
    let second = CacheManager::new(policy.clone(), second_store.clone());
    let key = policy.key(input()).unwrap();
    let tag = CacheTag::new("catalog-items").unwrap();
    for manager in [&first, &second] {
        manager
            .insert(
                &key,
                &CachedValue {
                    value: "old".to_owned(),
                },
                [tag.clone()],
            )
            .await
            .unwrap();
    }
    let event = coordinator
        .invalidate_tags(&policy, [tag], "rename-action")
        .unwrap();
    assert_eq!(event.sequence, 1);
    assert_eq!(event.expected_acknowledgements, 2);
    assert_eq!(event.acknowledged_replicas, 2);
    assert_eq!(event.removed_entries, 2);
    event.require_acknowledgements().unwrap();
    let explanation = event.explain();
    assert!(explanation.contains("acknowledged: 2/2"));
    assert!(
        first
            .lookup::<CachedValue>(&key)
            .await
            .unwrap()
            .value
            .is_none()
    );
    assert!(
        second
            .lookup::<CachedValue>(&key)
            .await
            .unwrap()
            .value
            .is_none()
    );

    let duplicate = coordinator
        .invalidate_tags(
            &policy,
            [CacheTag::new("catalog-items").unwrap()],
            "rename-action",
        )
        .unwrap();
    assert_eq!(duplicate, event);
    assert_eq!(duplicate.removed_entries, 2);
}

#[tokio::test]
async fn exact_key_invalidation_is_scoped_and_duplicate_delivery_is_idempotent() {
    let policy = public_policy();
    let store = InMemoryCacheStore::new(8).unwrap();
    let coordinator = InMemoryInvalidationCoordinator::new();
    coordinator.register(&store);
    let manager = CacheManager::new(policy.clone(), store);
    let first = policy.key(input()).unwrap();
    let second = policy
        .key(
            CacheKeyInput::new("catalog-loader", 1, "d".repeat(64))
                .unwrap()
                .vary("locale", "en-US")
                .unwrap(),
        )
        .unwrap();
    for (key, value) in [(&first, "first"), (&second, "second")] {
        manager
            .insert(
                key,
                &CachedValue {
                    value: value.to_owned(),
                },
                [],
            )
            .await
            .unwrap();
    }
    let event = coordinator
        .invalidate_key(&policy, &first, "operator-revalidate")
        .unwrap();
    assert_eq!(event.removed_entries, 1);
    assert!(
        manager
            .lookup::<CachedValue>(&first)
            .await
            .unwrap()
            .value
            .is_none()
    );
    assert_eq!(
        manager
            .lookup::<CachedValue>(&second)
            .await
            .unwrap()
            .value
            .unwrap()
            .value,
        "second"
    );
    assert_eq!(
        coordinator
            .invalidate_key(&policy, &first, "operator-revalidate")
            .unwrap(),
        event
    );
}

#[tokio::test]
async fn coalesced_fill_runs_once_and_cancelled_waiter_does_not_cancel_it() {
    let policy = public_policy()
        .stampede(true, Duration::from_secs(1))
        .unwrap();
    let manager = CacheManager::new(policy.clone(), InMemoryCacheStore::new(32).unwrap());
    let key = policy.key(input()).unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let (leader_context, leader_control) = request_context("cache-leader");
    let leader_manager = manager.clone();
    let leader_key = key.clone();
    let leader_executions = executions.clone();
    let leader = tokio::spawn(async move {
        leader_context
            .cache_get_or_fill(&leader_manager, &leader_key, Vec::new(), async move {
                leader_executions.fetch_add(1, Ordering::AcqRel);
                tokio::time::sleep(Duration::from_millis(30)).await;
                Ok::<_, CacheError>(CachedValue {
                    value: "filled".to_owned(),
                })
            })
            .await
    });
    tokio::time::sleep(Duration::from_millis(5)).await;

    let (waiter_context, waiter_control) = request_context("cache-waiter");
    let waiter_manager = manager.clone();
    let waiter_key = key.clone();
    let waiter = tokio::spawn(async move {
        waiter_context
            .cache_get_or_fill(&waiter_manager, &waiter_key, Vec::new(), async move {
                panic!("waiter fill must not execute");
                #[allow(unreachable_code)]
                Ok::<_, CacheError>(CachedValue {
                    value: "unreachable".to_owned(),
                })
            })
            .await
    });
    waiter_control.cancel(DataCancelReason::ApplicationAbort);
    assert_eq!(waiter.await.unwrap(), Err(CacheError::Cancelled));

    let completed = leader.await.unwrap().unwrap();
    assert_eq!(completed.value.unwrap().value, "filled");
    assert_eq!(executions.load(Ordering::Acquire), 1);
    let receipt = manager.lookup::<CachedValue>(&key).await.unwrap().receipt;
    let why = receipt.explain();
    assert!(why.contains("PLIEGO why cache"));
    assert!(!why.contains("filled"));
    leader_control.close();
}

#[tokio::test]
async fn compatibility_epoch_and_store_capacity_fail_closed() {
    let policy = public_policy();
    let store = InMemoryCacheStore::new(1).unwrap();
    let manager = CacheManager::new(policy.clone(), store.clone());
    let key = policy.key(input()).unwrap();
    manager
        .insert(
            &key,
            &CachedValue {
                value: "one".to_owned(),
            },
            [],
        )
        .await
        .unwrap();
    let second_key = policy
        .key(
            CacheKeyInput::new("catalog-loader", 1, "c".repeat(64))
                .unwrap()
                .vary("locale", "en-US")
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        manager
            .insert(
                &second_key,
                &CachedValue {
                    value: "two".to_owned()
                },
                []
            )
            .await,
        Err(CacheError::StoreCapacity)
    );

    let next_policy =
        CachePolicy::new("catalog-cache", 1, "catalog", 2, CacheDomain::PublicRuntime)
            .unwrap()
            .vary("locale")
            .unwrap();
    let next = CacheManager::new(next_policy, store);
    assert_eq!(
        next.lookup::<CachedValue>(&key).await,
        Err(CacheError::VersionMismatch)
    );
}
