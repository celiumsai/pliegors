// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pliego_fold::{
    CanonicalJsonCodec, CausalErrorKind, CausalLimits, CausalScheduler, DispatchOutcome, Reducer,
    ReducerError, ReducerIdentity, RuntimePhase, TransactionFailure, TransactionOutcome,
};
use pliego_log::{EventCatalogBuilder, EventSchema};
use pliego_reactive::Effect;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Delta {
    amount: i64,
}

impl EventSchema for Delta {
    const KIND: &'static str = "app_causal_delta";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "test.causal-delta/v1";
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Counter {
    value: i64,
    order: Vec<i64>,
}

fn scheduler() -> CausalScheduler<Counter, Delta> {
    scheduler_with_limits_and_reducer(CausalLimits::default(), |state, event| {
        if event.amount == 99 {
            return Err(ReducerError::new("injected rejection"));
        }
        if event.amount == 98 {
            panic!("injected reducer panic");
        }
        state.value += event.amount;
        state.order.push(event.amount);
        Ok(())
    })
}

fn scheduler_with_limits(limits: CausalLimits) -> CausalScheduler<Counter, Delta> {
    scheduler_with_limits_and_reducer(limits, |state, event| {
        if event.amount == 99 {
            return Err(ReducerError::new("injected rejection"));
        }
        state.value += event.amount;
        state.order.push(event.amount);
        Ok(())
    })
}

fn scheduler_with_limits_and_reducer(
    limits: CausalLimits,
    reduce: impl Fn(&mut Counter, &Delta) -> Result<(), ReducerError> + 'static,
) -> CausalScheduler<Counter, Delta> {
    let mut catalog = EventCatalogBuilder::new();
    catalog
        .register_current::<Delta, _>("test.causal-delta/current/1", |event| event)
        .unwrap();
    let identity = ReducerIdentity::from_serializable_config(
        "test.causal-counter",
        1,
        &serde_json::json!({ "mode": "sum" }),
    )
    .unwrap();
    CausalScheduler::with_limits(
        Counter::default(),
        catalog.seal().unwrap(),
        Reducer::new(identity, reduce),
        CanonicalJsonCodec::default(),
        limits,
    )
    .unwrap()
}

#[test]
fn reducer_panic_preserves_root_restores_idle_and_allows_recovery() {
    let scheduler = scheduler();
    let before = scheduler.state_root().unwrap();
    let error = scheduler.dispatch(Delta { amount: 98 }).unwrap_err();

    assert!(matches!(
        error.kind(),
        CausalErrorKind::ProjectionRejected(pliego_fold::ProjectionError::ReducerPanicked { .. })
    ));
    assert_eq!(error.receipts()[0].before, error.receipts()[0].after);
    assert_eq!(scheduler.state_root().unwrap(), before);
    assert_eq!(scheduler.phase(), RuntimePhase::Idle);
    assert!(scheduler.dispatch(Delta { amount: 1 }).is_ok());
}

#[test]
fn effect_panic_reports_committed_state_and_drains_healthy_n_plus_one_work() {
    let scheduler = scheduler();
    let healthy = scheduler.handle();
    let failing = scheduler.handle();
    let healthy_once = Rc::new(Cell::new(false));
    let panic_once = Rc::new(Cell::new(false));
    let healthy_gate = Rc::clone(&healthy_once);
    let panic_gate = Rc::clone(&panic_once);
    let healthy_effect = Effect::new(move || {
        if healthy.state().unwrap().value == 1 && !healthy_gate.replace(true) {
            healthy.enqueue(Delta { amount: 2 }).unwrap();
        }
    });
    let failing_effect = Effect::new(move || {
        if failing.state().unwrap().value == 1 && !panic_gate.replace(true) {
            panic!("injected effect panic");
        }
    });

    let error = scheduler.dispatch(Delta { amount: 1 }).unwrap_err();
    assert!(matches!(
        error.kind(),
        CausalErrorKind::EffectPanicked { transaction } if transaction.get() == 1
    ));
    assert_eq!(error.receipts().len(), 2);
    assert_eq!(error.receipts()[0].outcome, TransactionOutcome::Committed);
    assert_eq!(
        error.receipts()[0].failure,
        Some(TransactionFailure::EffectPanicked)
    );
    assert_eq!(error.receipts()[1].transaction_id.get(), 2);
    assert_eq!(scheduler.state().unwrap().order, [1, 2]);
    assert_eq!(scheduler.phase(), RuntimePhase::Idle);
    healthy_effect.dispose();
    failing_effect.dispose();
}

#[test]
fn dropped_scheduler_is_visible_to_weak_effect_handle() {
    let handle = {
        let scheduler = scheduler();
        scheduler.handle()
    };
    assert!(matches!(
        handle.state().unwrap_err().kind(),
        CausalErrorKind::SchedulerDropped
    ));
}

#[test]
fn single_dispatch_emits_a_root_bound_receipt() {
    let scheduler = scheduler();
    let before = scheduler.state_root().unwrap();
    let DispatchOutcome::Drained { ticket, report } =
        scheduler.dispatch(Delta { amount: 2 }).unwrap()
    else {
        panic!("root dispatch must drain");
    };

    assert_eq!(ticket.transaction.get(), 1);
    assert_eq!(report.receipts.len(), 1);
    let receipt = &report.receipts[0];
    assert_eq!(receipt.outcome, TransactionOutcome::Committed);
    assert_eq!(receipt.before.state_root, before);
    assert_eq!(receipt.after.state_root, scheduler.state_root().unwrap());
    assert_eq!(receipt.before.history.position, 0);
    assert_eq!(receipt.after.history.position, 1);
    assert_eq!(scheduler.state().unwrap().value, 2);
    assert_eq!(scheduler.phase(), RuntimePhase::Idle);
}

#[test]
fn effect_produced_events_execute_in_transaction_n_plus_one() {
    let scheduler = scheduler();
    let handle = scheduler.handle();
    let once = Rc::new(Cell::new(false));
    let observed_ticket = Rc::new(RefCell::new(None));
    let effect_once = Rc::clone(&once);
    let effect_ticket = Rc::clone(&observed_ticket);
    let effect = Effect::new(move || {
        let state = handle
            .state()
            .expect("scheduler remains live during effect");
        if state.value == 1 && !effect_once.replace(true) {
            *effect_ticket.borrow_mut() = Some(handle.enqueue(Delta { amount: 2 }).unwrap());
        }
    });

    let DispatchOutcome::Drained { report, .. } = scheduler.dispatch(Delta { amount: 1 }).unwrap()
    else {
        panic!("root dispatch must drain");
    };
    assert_eq!(report.receipts.len(), 2);
    assert_eq!(report.receipts[0].transaction_id.get(), 1);
    assert_eq!(report.receipts[0].after.history.position, 1);
    assert_eq!(report.receipts[1].transaction_id.get(), 2);
    assert_eq!(report.receipts[1].before, report.receipts[0].after);
    assert_eq!(report.receipts[1].after.history.position, 2);
    assert_eq!(observed_ticket.borrow().unwrap().transaction.get(), 2);
    assert_eq!(scheduler.state().unwrap().order, [1, 2]);
    effect.dispose();
}

#[test]
fn rejected_transaction_preserves_root_and_restores_idle() {
    let scheduler = scheduler();
    let before = scheduler.state_root().unwrap();
    let error = scheduler.dispatch(Delta { amount: 99 }).unwrap_err();

    assert!(matches!(
        error.kind(),
        CausalErrorKind::ProjectionRejected(_)
    ));
    assert_eq!(error.receipts().len(), 1);
    assert_eq!(error.receipts()[0].outcome, TransactionOutcome::Rejected);
    assert_eq!(
        error.receipts()[0].failure,
        Some(TransactionFailure::ProjectionRejected)
    );
    assert_eq!(error.receipts()[0].before, error.receipts()[0].after);
    assert_eq!(scheduler.state_root().unwrap(), before);
    assert_eq!(scheduler.history().unwrap().position, 0);
    assert_eq!(scheduler.phase(), RuntimePhase::Idle);
    assert!(scheduler.dispatch(Delta { amount: 3 }).is_ok());
}

#[test]
fn causal_chain_drains_iteratively_with_receipt_continuity() {
    let scheduler = scheduler();
    let handle = scheduler.handle();
    let effect = Effect::new(move || {
        let state = handle.state().unwrap();
        if state.value < 64 {
            handle.enqueue(Delta { amount: 1 }).unwrap();
        }
    });
    let report = scheduler.drain().unwrap();
    assert_eq!(report.receipts.len(), 64);
    assert_eq!(scheduler.state().unwrap().value, 64);
    assert!(
        report
            .receipts
            .windows(2)
            .all(|pair| pair[0].after == pair[1].before)
    );
    effect.dispose();
}

#[test]
fn transaction_limit_fails_closed_and_allows_later_work() {
    let scheduler = scheduler_with_limits(CausalLimits {
        max_events_per_transaction: 4,
        max_transactions_per_drain: 3,
        max_events_per_drain: 4,
    });
    let handle = scheduler.handle();
    let effect = Effect::new(move || {
        let state = handle.state().unwrap();
        if state.value < 10 {
            let _ = handle.enqueue(Delta { amount: 1 });
        }
    });
    let error = scheduler.drain().unwrap_err();
    assert!(matches!(
        error.kind(),
        CausalErrorKind::TransactionLimit { maximum: 3 }
    ));
    assert_eq!(scheduler.phase(), RuntimePhase::Idle);
    effect.dispose();
}

#[test]
fn receipt_json_is_bounded_and_uses_tagged_state_roots() {
    let scheduler = scheduler();
    let DispatchOutcome::Drained { report, .. } = scheduler.dispatch(Delta { amount: 1 }).unwrap()
    else {
        panic!("root dispatch must drain");
    };
    let json = serde_json::to_value(&report.receipts[0]).unwrap();
    assert_eq!(json["contract"], "dev.pliegors.fold-transaction-receipt/v1");
    assert_eq!(json["transactionId"], 1);
    assert_eq!(json["outcome"], "committed");
    assert_eq!(json["after"]["stateRoot"]["format"], 1);
    assert!(
        json["after"]["stateRoot"]["value"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert!(json.get("state").is_none());
}
