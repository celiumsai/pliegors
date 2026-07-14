# Resumable islands

**Status:** first interactive vertical slice

`pliego-resume` encodes initial state, bindings, and standard actions directly
into server-generated HTML. A delegated client module resumes only the island
that receives an event. It does not execute the component again, reconstruct a
reactive graph, walk the existing view, or hydrate the complete document.

```rust
let controls = view! {
    <section>
        {increment(el("button").child("+"), "minutes", 5)?}
        {text_binding("minutes", "15")?}
    </section>
};

let island = Island::new("ritual-counter")?
    .state_i64("minutes", 15)?
    .child(controls)
    .into_view()?;
```

The generated contract is ordinary inspectable HTML:

```html
<pliego-island
  data-pliego-id="ritual-counter"
  data-pliego-state="{&quot;minutes&quot;:15}">
  <!-- bindings and actions -->
</pliego-island>
```

## First gate

- A static route contains zero script elements.
- A resumed interactive route contains one module script.
- The uncompressed delegated runtime is 989 bytes.
- A real browser click changes the bound value from `15` to `20` and updates
  serialized island state from `{"minutes":15}` to `{"minutes":20}`.
- No console warning or error is produced.

## Current boundary

This slice intentionally supports only deterministic integer increment actions
and text bindings. It proves the resume contract, not the final action system.
The next increments must add typed reducer/action registration, event-log
append, persistent snapshots, lazy action modules, focus/input preservation,
and island disposal. Custom application logic must not be encoded as arbitrary
strings in HTML.

External libraries and custom imperative behavior belong in lifecycle-scoped
ESM adapters; see `docs/12-external-adapters.md`.
