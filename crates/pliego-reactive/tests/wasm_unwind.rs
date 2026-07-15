// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use pliego_reactive::{Effect, Memo, Signal};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn update_panic_rolls_back_and_runtime_remains_usable() {
    let signal = Signal::new(1);
    let failed = catch_unwind(AssertUnwindSafe(|| {
        signal.update(|value| {
            *value = 99;
            panic!("intentional WASM update panic");
        });
    }));

    assert!(failed.is_err());
    assert_eq!(signal.get_untracked(), 1);
    signal.update(|value| *value += 1);
    assert_eq!(signal.get_untracked(), 2);
}

#[wasm_bindgen_test]
fn scheduler_drains_healthy_work_before_resuming_effect_panic() {
    let trigger = Signal::new(0);
    let nested = Signal::new(0);
    let observed = Rc::new(Cell::new(0));
    let watcher = {
        let observed = observed.clone();
        Effect::new(move || observed.set(nested.get()))
    };
    let failing = Effect::new(move || {
        if trigger.get() == 1 {
            nested.set(7);
            panic!("intentional WASM effect panic");
        }
    });

    let failed = catch_unwind(AssertUnwindSafe(|| trigger.set(1)));
    assert!(failed.is_err());
    assert_eq!(observed.get(), 7);

    trigger.set(2);
    failing.dispose();
    watcher.dispose();
}

#[wasm_bindgen_test]
fn observer_is_restored_after_memo_panic() {
    let source = Signal::new(2);
    let panic_once = Rc::new(Cell::new(true));
    let memo = {
        let panic_once = panic_once.clone();
        Memo::new(move || {
            let value = source.get();
            if panic_once.replace(false) {
                panic!("intentional WASM memo panic");
            }
            value * 2
        })
    };

    assert!(catch_unwind(AssertUnwindSafe(|| memo.get())).is_err());
    assert_eq!(memo.get(), 4);
}

#[wasm_bindgen_test]
fn failed_memo_rearms_downstream_effect_on_next_invalidation() {
    let source = Signal::new(0);
    let fail = Rc::new(Cell::new(false));
    let first = {
        let fail = fail.clone();
        Memo::new(move || {
            let value = source.get();
            assert!(!fail.get(), "intentional WASM upstream memo panic");
            value
        })
    };
    let second = Memo::new(move || first.get() * 2);
    let seen = Rc::new(Cell::new(-1));
    let watcher = {
        let seen = seen.clone();
        Effect::new(move || seen.set(second.get()))
    };

    fail.set(true);
    assert!(catch_unwind(AssertUnwindSafe(|| source.set(1))).is_err());
    assert_eq!(seen.get(), 0);

    fail.set(false);
    source.set(2);
    assert_eq!(seen.get(), 4);
    watcher.dispose();
}

#[wasm_bindgen_test]
fn effect_feedback_cycle_is_bounded_and_runtime_recovers() {
    let x = Signal::new(0);
    let y = Signal::new(0);
    let enabled = Rc::new(Cell::new(false));
    let first = {
        let enabled = enabled.clone();
        Effect::new(move || {
            let value = x.get();
            if enabled.get() {
                y.set(value + 1);
            }
        })
    };
    let second = {
        let enabled = enabled.clone();
        Effect::new(move || {
            let value = y.get();
            if enabled.get() {
                x.set(value + 1);
            }
        })
    };

    enabled.set(true);
    assert!(catch_unwind(AssertUnwindSafe(|| x.set(1))).is_err());

    enabled.set(false);
    x.set(0);
    y.set(0);
    first.dispose();
    second.dispose();
}
