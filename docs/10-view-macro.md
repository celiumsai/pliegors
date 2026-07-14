# Typed `view!` RSX

**Status:** first compiler-facing contract

`pliego-macros::view!` parses RSX at compile time with `rstml` and emits the
same typed `pliego_dom::View` tree used by native HTML rendering and browser DOM
mounting. It does not render strings inside the macro and does not introduce a
second UI representation.

```rust
use pliego_macros::view;

let count = 3;
let page = view! {
    <main class="page">
        <h1>"Reference"</h1>
        <p data-count={count}>{count.to_string()}</p>
    </main>
};
```

## Supported now

- Native HTML elements and void elements.
- Literal and Rust-expression attribute values.
- Text, fragments, and Rust expression blocks as children.
- Dashed names such as `aria-label` and `data-count`.
- Compile-time rejection of unsupported node forms.

## Typed components

`#[component]` converts a PascalCase Rust function into a typed component and
generates its public `NameProps` contract. Component tags construct that props
value at compile time and pass nested RSX as `children`.

```rust
#[component]
fn BrandLockup(label: String, children: View) -> View {
    view! {
        <nav>{children}<span>{label}</span></nav>
    }
}

let header = view! {
    <BrandLockup label="REFERENCE SITE">
        <strong>"PliegoRS"</strong>
    </BrandLockup>
};
```

The first component contract deliberately keeps every declared prop required.
Missing or mistyped props fail in Rust compilation; duplicate props and an
explicit `children=` prop fail inside macro expansion.

## Intentionally pending

- Optional/default props and prop shorthand.
- Generic, async, and qualified-path components.
- Event syntax and delegated event IDs.
- Reactive attribute/text bindings.
- Spread attributes.
- Resumable-island boundaries.

Those forms remain disabled until their emitted metadata and ownership model are
specified. The public reference migrated from manual builders to `view!`
without changing any generated file hash.
