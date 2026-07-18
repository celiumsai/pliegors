// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use js_sys::WebAssembly;
use pliego_dom::{IntoView, View, dyn_text, mount, text};
use pliego_reactive::Signal;
use serde::Serialize;
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};

const MAX_SAMPLES: u32 = 100;
const MAX_UPDATES: u32 = 100_000;
const MAX_PLATEAU_CYCLES: u32 = 100_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserObservation {
    sample: u32,
    total_ms: f64,
    per_update_us: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryObservation {
    completed_cycles: u32,
    linear_memory_bytes: u64,
    dom_child_nodes: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserBenchmark {
    contract: &'static str,
    update_samples: u32,
    updates_per_sample: u32,
    update_warmup: u32,
    observations: Vec<BrowserObservation>,
    plateau_warmup_cycles: u32,
    plateau_batch_cycles: u32,
    memory_observations: Vec<MemoryObservation>,
    memory_plateau: bool,
}

fn input_error(message: &str) -> JsValue {
    JsValue::from_str(message)
}

fn wasm_memory_bytes() -> Result<u64, JsValue> {
    let memory = wasm_bindgen::memory().dyn_into::<WebAssembly::Memory>()?;
    let buffer = memory.buffer().dyn_into::<js_sys::ArrayBuffer>()?;
    Ok(u64::from(buffer.byte_length()))
}

/// Measure synchronous PliegoRS DOM updates and repeated mount/dispose cycles.
#[wasm_bindgen]
pub fn run_browser_benchmark(
    samples: u32,
    updates_per_sample: u32,
    update_warmup: u32,
    plateau_batches: u32,
    plateau_batch_cycles: u32,
) -> Result<String, JsValue> {
    if samples == 0 || samples > MAX_SAMPLES {
        return Err(input_error("samples must be between 1 and 100"));
    }
    if updates_per_sample == 0 || updates_per_sample > MAX_UPDATES {
        return Err(input_error(
            "updates_per_sample must be between 1 and 100000",
        ));
    }
    let plateau_cycles = plateau_batches
        .checked_mul(plateau_batch_cycles)
        .ok_or_else(|| input_error("plateau cycle count overflow"))?;
    if plateau_batches < 3 || plateau_batch_cycles == 0 || plateau_cycles > MAX_PLATEAU_CYCLES {
        return Err(input_error(
            "plateau requires at least three non-empty batches and at most 100000 cycles",
        ));
    }

    let window = web_sys::window().ok_or_else(|| input_error("window is unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| input_error("document is unavailable"))?;
    let body = document
        .body()
        .ok_or_else(|| input_error("document body is unavailable"))?;
    let performance = window
        .performance()
        .ok_or_else(|| input_error("Performance API is unavailable"))?;

    let host = document.create_element("div")?;
    body.append_child(&host)?;
    let value = Signal::new(0_u32);
    let view = dyn_text(move || value.get().to_string()).into_view();
    let mounted = mount(&view, host.as_ref()).map_err(|error| input_error(&error.to_string()))?;
    for next in 1..=update_warmup {
        value.set(next);
    }

    let mut observations = Vec::with_capacity(samples as usize);
    let mut next = update_warmup;
    for sample in 1..=samples {
        let start = performance.now();
        for _ in 0..updates_per_sample {
            next = next.wrapping_add(1);
            value.set(next);
        }
        let total_ms = performance.now() - start;
        let expected = next.to_string();
        if host.text_content().as_deref() != Some(expected.as_str()) {
            return Err(input_error(
                "browser DOM did not apply the final signal value",
            ));
        }
        observations.push(BrowserObservation {
            sample,
            total_ms,
            per_update_us: total_ms * 1_000.0 / f64::from(updates_per_sample),
        });
    }
    mounted.dispose();
    value.dispose();
    host.remove();

    let plateau_host = document.create_element("div")?;
    body.append_child(&plateau_host)?;
    let plateau_view: View = text("plateau");
    for _ in 0..plateau_batch_cycles {
        let root = mount(&plateau_view, plateau_host.as_ref())
            .map_err(|error| input_error(&error.to_string()))?;
        root.dispose();
    }
    let mut memory_observations = Vec::with_capacity(plateau_batches as usize + 1);
    memory_observations.push(MemoryObservation {
        completed_cycles: plateau_batch_cycles,
        linear_memory_bytes: wasm_memory_bytes()?,
        dom_child_nodes: plateau_host.child_nodes().length(),
    });
    for batch in 1..=plateau_batches {
        for _ in 0..plateau_batch_cycles {
            let root = mount(&plateau_view, plateau_host.as_ref())
                .map_err(|error| input_error(&error.to_string()))?;
            root.dispose();
        }
        memory_observations.push(MemoryObservation {
            completed_cycles: plateau_batch_cycles * (batch + 1),
            linear_memory_bytes: wasm_memory_bytes()?,
            dom_child_nodes: plateau_host.child_nodes().length(),
        });
    }
    plateau_host.remove();

    let stable_tail = memory_observations
        .iter()
        .rev()
        .take(3)
        .map(|observation| observation.linear_memory_bytes)
        .collect::<Vec<_>>();
    let memory_plateau = stable_tail.len() == 3
        && stable_tail.windows(2).all(|pair| pair[0] == pair[1])
        && memory_observations
            .iter()
            .all(|observation| observation.dom_child_nodes == 0);

    serde_json::to_string(&BrowserBenchmark {
        contract: "dev.pliegors.browser-apply-benchmark/v1",
        update_samples: samples,
        updates_per_sample,
        update_warmup,
        observations,
        plateau_warmup_cycles: plateau_batch_cycles,
        plateau_batch_cycles,
        memory_observations,
        memory_plateau,
    })
    .map_err(|error| input_error(&format!("cannot serialize browser benchmark: {error}")))
}
