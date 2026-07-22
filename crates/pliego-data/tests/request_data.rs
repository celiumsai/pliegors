// SPDX-License-Identifier: Apache-2.0

use pliego_data::{
    ActionAdmission, ActionCommitState, ActionIdempotency, ActionInvalidationIntent,
    ActionMediaType, ActionNavigation, ActionPolicy, ActionResponse, CacheDomain, CachePolicy,
    CacheTag, CapabilitySet, DataCancelReason, DataContext, DataContextOptions, DataError,
    DataIdentity, DataRequestValues, IdempotencyKey, IdempotencyManager, IdempotencyPartition,
    IdempotencyPolicy, InMemoryIdempotencyStore, InMemoryInvalidationCoordinator, LoaderPolicy,
    ResourceGrant, ResourceRegistryBuilder, ResourceRequirement, ResourceSpec,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Query {
    item: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Record {
    value: String,
}

fn request_context() -> (DataContext, pliego_data::DataContextControl) {
    let capabilities = CapabilitySet::none()
        .allowing("read")
        .unwrap()
        .allowing("write")
        .unwrap();
    let resources = ResourceRegistryBuilder::new()
        .register(
            ResourceSpec::new("catalog", "memory-provider")
                .unwrap()
                .with_capabilities(capabilities),
            BTreeMap::from([(7_u32, "redacted-value".to_owned())]),
        )
        .unwrap()
        .seal();
    let grant = ResourceGrant::new("catalog")
        .unwrap()
        .allowing("read")
        .unwrap();
    DataContext::open(
        DataIdentity::new("request-1", "items", "deployment-1").unwrap(),
        Instant::now() + Duration::from_secs(5),
        resources,
        [grant],
        DataRequestValues::default(),
        DataContextOptions::default(),
    )
    .unwrap()
}

#[test]
fn resource_leases_require_route_grants_capabilities_and_types() {
    let (context, control) = request_context();
    let readable = ResourceRequirement::new("catalog")
        .unwrap()
        .requiring("read")
        .unwrap();
    let lease = context
        .resource::<BTreeMap<u32, String>>(&readable)
        .unwrap();
    assert_eq!(lease.get().unwrap().get(&7).unwrap(), "redacted-value");

    let writable = ResourceRequirement::new("catalog")
        .unwrap()
        .requiring("write")
        .unwrap();
    assert!(matches!(
        context.resource::<BTreeMap<u32, String>>(&writable),
        Err(DataError::MissingCapability { .. })
    ));
    assert!(matches!(
        context.resource::<Vec<String>>(&readable),
        Err(DataError::ResourceTypeMismatch(_))
    ));

    control.close();
    assert_eq!(lease.get(), Err(DataError::ContextClosed));
}

#[tokio::test]
async fn identical_loader_invocations_execute_once_and_publish_immutable_output() {
    let (context, control) = request_context();
    let calls = Arc::new(AtomicUsize::new(0));
    let loader_calls = calls.clone();
    let loader = move |loader_context: pliego_data::LoaderContext, query: Query| {
        let calls = loader_calls.clone();
        async move {
            calls.fetch_add(1, Ordering::AcqRel);
            let lease = loader_context.resource::<BTreeMap<u32, String>>("catalog")?;
            let value = lease
                .get()?
                .get(&query.item)
                .cloned()
                .ok_or_else(|| DataError::LoaderInput("unknown item".to_owned()))?;
            Ok(Record { value })
        }
    };
    let policy = LoaderPolicy::new("item-loader", 1, "item-query", "item-record")
        .unwrap()
        .resource(
            ResourceRequirement::new("catalog")
                .unwrap()
                .requiring("read")
                .unwrap(),
        )
        .unwrap();

    let (first, second) = tokio::join!(
        context.load(&policy, &loader, Query { item: 7 }),
        context.load(&policy, &loader, Query { item: 7 })
    );
    assert_eq!(first.unwrap().value, "redacted-value");
    assert_eq!(second.unwrap().value, "redacted-value");
    assert_eq!(calls.load(Ordering::Acquire), 1);
    let receipts = context.receipts();
    assert_eq!(
        receipts
            .iter()
            .filter(|receipt| receipt.operation_id == "item-loader")
            .count(),
        2
    );
    assert!(receipts.iter().any(|receipt| receipt.deduplicated));
    control.close();
}

#[tokio::test]
async fn input_revision_and_output_bounds_are_part_of_loader_correctness() {
    let (context, control) = request_context();
    let calls = Arc::new(AtomicUsize::new(0));
    let loader_calls = calls.clone();
    let loader = move |_context: pliego_data::LoaderContext, query: Query| {
        let calls = loader_calls.clone();
        async move {
            calls.fetch_add(1, Ordering::AcqRel);
            Ok(Record {
                value: "x".repeat(query.item as usize),
            })
        }
    };
    let policy = LoaderPolicy::new("bounded-loader", 1, "item-query", "item-record")
        .unwrap()
        .max_output_bytes(32)
        .unwrap();
    context
        .load(&policy, &loader, Query { item: 1 })
        .await
        .unwrap();
    context
        .load(&policy, &loader, Query { item: 2 })
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 2);
    assert!(matches!(
        context.load(&policy, &loader, Query { item: 100 }).await,
        Err(DataError::LoaderOutput { .. })
    ));
    control.close();
}

#[tokio::test]
async fn cancellation_interrupts_loader_and_cleanup_is_lifo() {
    let (context, control) = request_context();
    let order = Arc::new(Mutex::new(Vec::new()));
    for value in [1, 2, 3] {
        let order = order.clone();
        context
            .register_cleanup(move |reason| {
                assert_eq!(reason, DataCancelReason::ApplicationAbort);
                order.lock().unwrap().push(value);
                Ok(())
            })
            .unwrap();
    }
    let loader = |_context: pliego_data::LoaderContext, _query: Query| async move {
        std::future::pending::<Result<Record, DataError>>().await
    };
    let policy = LoaderPolicy::new("pending-loader", 1, "item-query", "item-record").unwrap();
    let load = context.load(&policy, &loader, Query { item: 1 });
    tokio::pin!(load);
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(10)) => {
            control.cancel(DataCancelReason::ApplicationAbort);
        }
        result = &mut load => panic!("loader completed before cancellation: {result:?}"),
    }
    assert_eq!(load.await, Err(DataError::Cancelled));
    control.close();
    assert_eq!(*order.lock().unwrap(), vec![3, 2, 1]);
}

#[test]
fn diagnostics_and_debug_views_do_not_expose_resource_values() {
    let (context, control) = request_context();
    let readable = ResourceRequirement::new("catalog")
        .unwrap()
        .requiring("read")
        .unwrap();
    let lease = context
        .resource::<BTreeMap<u32, String>>(&readable)
        .unwrap();
    let output = format!("{context:?} {lease:?} {:?}", context.receipts());
    assert!(!output.contains("redacted-value"));
    control.close();
}

fn action_policy() -> ActionPolicy {
    ActionPolicy::new(
        "rename-account",
        1,
        "rename-account-input",
        "rename-account-errors",
        "rename-account-output",
    )
    .unwrap()
}

fn admitted_form() -> ActionAdmission {
    ActionAdmission::new(ActionMediaType::FormUrlencoded, 64)
        .same_origin(true)
        .csrf_verified(true)
        .authenticated(true)
        .authorized(true)
}

#[tokio::test]
async fn progressive_action_uses_one_policy_for_form_success_and_field_errors() {
    let (context, control) = request_context();
    let action = |action_context: pliego_data::ActionContext, query: Query| async move {
        if query.item == 0 {
            return Ok::<_, DataError>(
                ActionResponse::<Record, BTreeMap<String, String>>::Invalid {
                    field_errors: BTreeMap::from([(
                        "item".to_owned(),
                        "must be greater than zero".to_owned(),
                    )]),
                },
            );
        }
        action_context.commit().enter_pre_commit()?;
        action_context.commit().begin_commit()?;
        action_context.commit().committed()?;
        Ok(ActionResponse::Success {
            output: Record {
                value: format!("account-{}", query.item),
            },
            navigation: ActionNavigation::SeeOther("/account".to_owned()),
        })
    };
    let policy = action_policy();

    let invalid = context
        .act(&policy, &admitted_form(), &action, Query { item: 0 })
        .await
        .unwrap();
    assert!(matches!(invalid, ActionResponse::Invalid { .. }));

    let success = context
        .act(&policy, &admitted_form(), &action, Query { item: 7 })
        .await
        .unwrap();
    assert_eq!(
        success,
        ActionResponse::Success {
            output: Record {
                value: "account-7".to_owned()
            },
            navigation: ActionNavigation::SeeOther("/account".to_owned())
        }
    );
    assert_eq!(
        context
            .receipts()
            .iter()
            .filter(|receipt| receipt.operation_id == "rename-account")
            .count(),
        2
    );
    control.close();
}

#[tokio::test]
async fn action_admission_fails_closed_for_origin_csrf_auth_and_media_type() {
    let (context, control) = request_context();
    let calls = Arc::new(AtomicUsize::new(0));
    let action_calls = calls.clone();
    let action = move |_context: pliego_data::ActionContext, _query: Query| {
        let calls = action_calls.clone();
        async move {
            calls.fetch_add(1, Ordering::AcqRel);
            Ok::<_, DataError>(
                ActionResponse::<Record, BTreeMap<String, String>>::Success {
                    output: Record {
                        value: "should-not-run".to_owned(),
                    },
                    navigation: ActionNavigation::Stay,
                },
            )
        }
    };
    let policy = action_policy();
    for admission in [
        ActionAdmission::new(ActionMediaType::FormUrlencoded, 64),
        admitted_form().same_origin(false),
        admitted_form().csrf_verified(false),
        admitted_form().authenticated(false),
        admitted_form().authorized(false),
        ActionAdmission::new(ActionMediaType::Json, 64)
            .same_origin(true)
            .csrf_verified(true)
            .authenticated(true)
            .authorized(true),
    ] {
        let failure = context
            .act(&policy, &admission, &action, Query { item: 7 })
            .await
            .unwrap_err();
        assert_eq!(failure.error().code(), "PLG-ACT-101");
        assert_eq!(failure.commit_state(), ActionCommitState::NotStarted);
    }
    assert_eq!(calls.load(Ordering::Acquire), 0);
    control.close();
}

#[tokio::test]
async fn cancellation_before_and_during_commit_never_claims_a_rollback() {
    let (context, control) = request_context();
    let pending = |_context: pliego_data::ActionContext, _query: Query| async move {
        std::future::pending::<Result<ActionResponse<Record, BTreeMap<String, String>>, DataError>>(
        )
        .await
    };
    let policy = action_policy();
    let admission = admitted_form();
    let action = context.act(&policy, &admission, &pending, Query { item: 1 });
    tokio::pin!(action);
    tokio::time::sleep(Duration::from_millis(5)).await;
    control.cancel(DataCancelReason::ApplicationAbort);
    let failure = action.await.unwrap_err();
    assert_eq!(failure.error(), &DataError::Cancelled);
    assert_eq!(failure.commit_state(), ActionCommitState::NotStarted);
    control.close();

    let (context, control) = request_context();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered_tx = Arc::new(Mutex::new(Some(entered_tx)));
    let action_sender = entered_tx.clone();
    let committing = move |action_context: pliego_data::ActionContext, _query: Query| {
        let sender = action_sender.clone();
        async move {
            action_context.commit().begin_commit()?;
            if let Some(sender) = sender.lock().unwrap().take() {
                let _ = sender.send(());
            }
            std::future::pending::<
                Result<ActionResponse<Record, BTreeMap<String, String>>, DataError>,
            >()
            .await
        }
    };
    let admission = admitted_form();
    let action = context.act(&policy, &admission, &committing, Query { item: 1 });
    tokio::pin!(action);
    tokio::select! {
        result = entered_rx => result.unwrap(),
        _ = &mut action => panic!("action completed before entering commit"),
    }
    control.cancel(DataCancelReason::ApplicationAbort);
    let failure = action.await.unwrap_err();
    assert_eq!(failure.error(), &DataError::ActionOutcomeUnknown);
    assert_eq!(failure.commit_state(), ActionCommitState::OutcomeUnknown);
    control.close();
}

#[tokio::test]
async fn cancellation_after_commit_allows_bounded_result_completion() {
    let (context, control) = request_context();
    let (committed_tx, committed_rx) = tokio::sync::oneshot::channel();
    let committed_tx = Arc::new(Mutex::new(Some(committed_tx)));
    let action_sender = committed_tx.clone();
    let committed = move |action_context: pliego_data::ActionContext, _query: Query| {
        let sender = action_sender.clone();
        async move {
            action_context.commit().begin_commit()?;
            action_context.commit().committed()?;
            if let Some(sender) = sender.lock().unwrap().take() {
                let _ = sender.send(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, DataError>(
                ActionResponse::<Record, BTreeMap<String, String>>::Success {
                    output: Record {
                        value: "committed".to_owned(),
                    },
                    navigation: ActionNavigation::Stay,
                },
            )
        }
    };
    let policy = action_policy();
    let admission = admitted_form();
    let action = context.act(&policy, &admission, &committed, Query { item: 1 });
    tokio::pin!(action);
    tokio::select! {
        result = committed_rx => result.unwrap(),
        _ = &mut action => panic!("action completed before commit signal"),
    }
    control.cancel(DataCancelReason::ClientDisconnect);
    let response = action.await.unwrap();
    assert!(matches!(response, ActionResponse::Success { .. }));
    control.close();
}

#[tokio::test]
async fn idempotent_action_replays_the_committed_result_without_mutating_twice() {
    let idempotency_policy = IdempotencyPolicy::new("standard-retry").unwrap();
    let manager =
        IdempotencyManager::new(idempotency_policy.clone(), InMemoryIdempotencyStore::new());
    let policy = action_policy().idempotency(&idempotency_policy);
    let calls = Arc::new(AtomicUsize::new(0));
    let action_calls = calls.clone();
    let action = move |context: pliego_data::ActionContext, query: Query| {
        let calls = action_calls.clone();
        async move {
            calls.fetch_add(1, Ordering::AcqRel);
            context.commit().begin_commit()?;
            context.commit().committed()?;
            Ok::<_, DataError>(
                ActionResponse::<Record, BTreeMap<String, String>>::Success {
                    output: Record {
                        value: format!("account-{}", query.item),
                    },
                    navigation: ActionNavigation::Stay,
                },
            )
        }
    };
    let key = IdempotencyKey::parse("rename-request-00000001").unwrap();
    let partition = IdempotencyPartition::from_identity("user-1").unwrap();
    for _ in 0..2 {
        let (context, control) = request_context();
        let response = context
            .act_idempotent(
                &policy,
                &admitted_form(),
                &action,
                Query { item: 7 },
                ActionIdempotency::new(&manager, &key, &partition, 1).unwrap(),
            )
            .await
            .unwrap();
        assert!(matches!(response, ActionResponse::Success { .. }));
        control.close();
    }
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn action_context_enforces_declared_invalidation_and_acknowledgement_barrier() {
    let cache_policy = CachePolicy::new(
        "account-private",
        1,
        "accounts",
        1,
        CacheDomain::PrivateSession,
    )
    .unwrap();
    let coordinator = InMemoryInvalidationCoordinator::new();
    let mut event = coordinator
        .invalidate_tags(
            &cache_policy,
            [CacheTag::new("account-private").unwrap()],
            "request-action",
        )
        .unwrap();
    event.expected_acknowledgements = 1;
    let declared = action_policy()
        .invalidation(
            ActionInvalidationIntent::tags(
                "account-private",
                [CacheTag::new("account-private").unwrap()],
            )
            .unwrap()
            .read_your_writes(),
        )
        .unwrap();
    let action = move |context: pliego_data::ActionContext, _query: Query| {
        let event = event.clone();
        async move {
            assert!(context.record_invalidation(event.clone()).is_err());
            context.commit().begin_commit()?;
            context.commit().committed()?;
            context.record_invalidation(event)?;
            Ok::<_, DataError>(
                ActionResponse::<Record, BTreeMap<String, String>>::Success {
                    output: Record {
                        value: "unreachable".to_owned(),
                    },
                    navigation: ActionNavigation::Stay,
                },
            )
        }
    };
    let (context, control) = request_context();
    let failure = context
        .act(&declared, &admitted_form(), &action, Query { item: 1 })
        .await
        .unwrap_err();
    assert!(matches!(failure.error(), DataError::ActionFailure(_)));
    assert_eq!(failure.commit_state(), ActionCommitState::Committed);
    assert!(context.invalidation_events().is_empty());
    control.close();

    let wrong_target = coordinator
        .invalidate_tags(
            &cache_policy,
            [CacheTag::new("account-other").unwrap()],
            "request-wrong-target",
        )
        .unwrap();
    let action = move |context: pliego_data::ActionContext, _query: Query| {
        let event = wrong_target.clone();
        async move {
            context.commit().begin_commit()?;
            context.commit().committed()?;
            context.record_invalidation(event)?;
            Ok::<_, DataError>(
                ActionResponse::<Record, BTreeMap<String, String>>::Success {
                    output: Record {
                        value: "unreachable".to_owned(),
                    },
                    navigation: ActionNavigation::Stay,
                },
            )
        }
    };
    let (context, control) = request_context();
    let failure = context
        .act(&declared, &admitted_form(), &action, Query { item: 1 })
        .await
        .unwrap_err();
    assert!(matches!(
        failure.error(),
        DataError::InvalidActionState(message) if message.contains("target does not match")
    ));
    assert!(context.invalidation_events().is_empty());
    control.close();

    let undeclared_event = coordinator
        .invalidate_tags(
            &cache_policy,
            [CacheTag::new("account-private").unwrap()],
            "request-undeclared",
        )
        .unwrap();
    let action = move |context: pliego_data::ActionContext, _query: Query| {
        let event = undeclared_event.clone();
        async move {
            context.commit().begin_commit()?;
            context.commit().committed()?;
            context.record_invalidation(event)?;
            Ok::<_, DataError>(
                ActionResponse::<Record, BTreeMap<String, String>>::Success {
                    output: Record {
                        value: "unreachable".to_owned(),
                    },
                    navigation: ActionNavigation::Stay,
                },
            )
        }
    };
    let (context, control) = request_context();
    let failure = context
        .act(
            &action_policy(),
            &admitted_form(),
            &action,
            Query { item: 1 },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        failure.error(),
        DataError::InvalidActionState(message) if message.contains("not declared")
    ));
    assert!(context.invalidation_events().is_empty());
    control.close();
}
