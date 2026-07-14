// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Declarative resumable islands.
//!
//! Standard actions are encoded into SSR HTML. The client runtime resumes only
//! the island state touched by an event; it does not execute or hydrate a view.

use pliego_dom::{Element, IntoView, View, el};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

pub const RUNTIME_PATH: &str = "assets/pliego-resume.js";

pub const RUNTIME_JS: &str = r#"const states=new WeakMap();
function stateFor(island){let state=states.get(island);if(state)return state;state=JSON.parse(island.dataset.pliegoState||'{}');states.set(island,state);return state}
function render(island,key,value){for(const node of island.querySelectorAll(`[data-pliego-bind-text="${CSS.escape(key)}"]`))node.textContent=String(value)}
document.addEventListener('click',event=>{const origin=event.target;if(!(origin instanceof Element))return;const control=origin.closest('[data-pliego-action]');if(!control)return;const island=control.closest('pliego-island');if(!island)return;const action=control.dataset.pliegoAction;const key=control.dataset.pliegoKey;const state=stateFor(island);if(action==='increment'){const by=Number(control.dataset.pliegoBy||1);state[key]=Number(state[key]||0)+by;render(island,key,state[key]);island.dataset.pliegoState=JSON.stringify(state);island.dispatchEvent(new CustomEvent('pliego:state',{bubbles:true,detail:{key,value:state[key]}}))}});
"#;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResumeError(String);

impl fmt::Display for ResumeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ResumeError {}

pub struct Island {
    id: String,
    state: BTreeMap<String, Value>,
    child: Option<View>,
}

impl Island {
    pub fn new(id: impl Into<String>) -> Result<Self, ResumeError> {
        let id = id.into();
        validate_identifier("island", &id)?;
        Ok(Self {
            id,
            state: BTreeMap::new(),
            child: None,
        })
    }

    pub fn state_i64(mut self, key: impl Into<String>, value: i64) -> Result<Self, ResumeError> {
        let key = key.into();
        validate_identifier("state key", &key)?;
        self.state.insert(key, value.into());
        Ok(self)
    }

    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(child.into_view());
        self
    }

    pub fn into_view(self) -> Result<View, ResumeError> {
        let state = serde_json::to_string(&self.state)
            .map_err(|error| ResumeError(format!("cannot serialize island state: {error}")))?;
        Ok(el("pliego-island")
            .attr("data-pliego-id", self.id)
            .attr("data-pliego-state", state)
            .child(self.child.unwrap_or_else(|| View::Fragment(Vec::new())))
            .into_view())
    }
}

pub fn increment(
    element: Element,
    state_key: impl Into<String>,
    by: i64,
) -> Result<Element, ResumeError> {
    let state_key = state_key.into();
    validate_identifier("state key", &state_key)?;
    Ok(element
        .attr("data-pliego-action", "increment")
        .attr("data-pliego-key", state_key)
        .attr("data-pliego-by", by.to_string()))
}

pub fn text_binding(
    state_key: impl Into<String>,
    initial: impl Into<String>,
) -> Result<Element, ResumeError> {
    let state_key = state_key.into();
    validate_identifier("state key", &state_key)?;
    Ok(el("span")
        .attr("data-pliego-bind-text", state_key)
        .child(initial.into()))
}

pub fn runtime_bytes() -> Vec<u8> {
    RUNTIME_JS.as_bytes().to_vec()
}

fn validate_identifier(kind: &str, value: &str) -> Result<(), ResumeError> {
    let mut characters = value.chars();
    let valid_start = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic());
    let valid_tail = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid_start || !valid_tail {
        return Err(ResumeError(format!(
            "invalid {kind} {value:?}; expected an ASCII letter followed by letters, digits, '-' or '_'"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_dom::render_html;

    #[test]
    fn island_state_and_actions_are_deterministic_html() {
        let button = increment(el("button").child("more"), "minutes", 5).unwrap();
        let view = Island::new("ritual-counter")
            .unwrap()
            .state_i64("minutes", 15)
            .unwrap()
            .child(button)
            .into_view()
            .unwrap();
        assert_eq!(
            render_html(&view),
            r#"<pliego-island data-pliego-id="ritual-counter" data-pliego-state="{&quot;minutes&quot;:15}"><button data-pliego-action="increment" data-pliego-key="minutes" data-pliego-by="5">more</button></pliego-island>"#
        );
    }

    #[test]
    fn identifiers_reject_selector_and_attribute_injection() {
        assert!(Island::new("bad id").is_err());
        assert!(text_binding("x\"]", "0").is_err());
    }
}
