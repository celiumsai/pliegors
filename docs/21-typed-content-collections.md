# Typed content collections

**Status:** AU-01 and AU-01B accepted on 2026-07-12
**Package:** `pliego-content`
**Executable proof:** `examples/content-collections-pliegors`

PliegoRS content collections turn authored files into deterministic typed Rust
inputs. The package owns discovery, parsing, security policy, diagnostics,
portable identity, and change snapshots. It deliberately does not own routes,
HTML, components, SSG output, caching, or deployment.

That separation keeps `pliego-content` usable by static generation, future SSR,
Hyphae projections, build tooling, and other consumers without importing
`pliego-dom`, `pliego-ssg`, or `pliego-cli`.

## Formats

One directory-backed collection may contain:

- Markdown or `.markdown` with required frontmatter;
- JSON, where one file is one typed entry;
- TOML, where one file is one typed entry;
- nested directories, which become slash-separated entry IDs.

New PliegoRS Markdown uses TOML frontmatter with `+++` delimiters. YAML with
`---` delimiters is an explicit migration compatibility surface. The delimiter
must be alone on its line and must close before the CommonMark body.

```markdown
+++
title = "First fold"
published = true
+++

The body remains authored **CommonMark**.
```

## API

```rust
use pliego_content::{CollectionSpec, MarkdownPolicy};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Article {
    title: String,
    published: bool,
}

let articles = CollectionSpec::new("content/articles")
    .options(|options| options.markdown_policy(MarkdownPolicy::Safe))
    .load::<Article>()?;

for entry in &articles {
    println!("{}: {}", entry.id(), entry.data().title);
}
# Ok::<(), pliego_content::LoadError>(())
```

`T` only requires `DeserializeOwned`. The framework does not add `Clone`,
`Serialize`, default values, route fields, slugs, or locale behavior to the
project schema. Serde attributes and the project's Rust type remain the schema.

An `Entry<T>` exposes its portable ID, source path, relative path, source
format, frontmatter format, typed data, optional Markdown document, and content
fingerprint. `Collection<T>` is sorted by ID and supports deterministic
iteration and binary lookup.

## Resource budgets

Every load has finite resource ceilings, including consumers that continue to
call `CollectionSpec::new(root).load()` without explicit options. The secure
defaults are:

| Budget | Default |
| --- | ---: |
| Directory depth below root | 32 |
| Filesystem entries visited | 10,000 |
| Bytes in one content source | 8 MiB |
| Bytes across content sources | 128 MiB |

The entry budget counts directories and ignored files as well as supported
content sources. This is intentional: every directory entry consumes traversal
time and memory even when it does not become a `Collection` entry. Depth zero
allows regular files directly in the root and rejects every child directory.

Projects with a deliberately larger trusted corpus can replace the complete
budget without disabling the other loader policies:

```rust
use pliego_content::{CollectionSpec, ContentLimits, LoadOptions};

let limits = ContentLimits::new()
    .max_depth(12)
    .max_entries(25_000)
    .max_file_bytes(4 * 1024 * 1024)
    .max_total_bytes(96 * 1024 * 1024);
let articles = CollectionSpec::new("content/articles")
    .with_options(LoadOptions::new().limits(limits))
    .load::<serde_json::Value>()?;
# Ok::<(), pliego_content::LoadError>(())
```

Discovery stops at the first resource-ceiling violation, before schema or
Markdown parsing. File sizes and their aggregate are checked from metadata
during deterministic traversal. Each file is then checked again and read
through a bounded reader, so growth between discovery and opening cannot turn
into an unbounded allocation. Resource failures use stable machine codes:

- `content.limit.depth`
- `content.limit.entries`
- `content.limit.file_bytes`
- `content.limit.total_bytes`

Resource budgets do not participate in entry fingerprints: they govern whether
a corpus may load, not the content identity of a source that loaded.

## Markdown boundary

`MarkdownDocument` exposes an owned, parser-independent CommonMark event stream.
No pulldown-cmark type crosses the public contract, and no trusted HTML node is
added to `pliego-dom`.

`MarkdownPolicy::Safe` is the default. It rejects raw HTML and URI schemes other
than relative references, fragments, HTTP(S), mail, and telephone links with a
typed span. `MarkdownPolicy::Trusted` is an explicit author-trust decision; it
does not bypass the escaped PliegoRS DOM renderer by itself.

The executable proof maps safe Markdown events into authored `pliego-dom`
elements. Applications remain responsible for deciding how headings, lists,
links, images, code, and prose become components.

## Determinism

Collection behavior is independent of filesystem enumeration order:

1. Discover without following symbolic links or Windows reparse points.
2. Reject non-portable paths and case-insensitive ID collisions.
3. Require strict UTF-8 and reject byte-order marks.
4. Normalize CRLF to LF before parsing and fingerprinting.
5. Sort entries by portable slash-separated ID.
6. Aggregate diagnostics in stable path, code, and span order.

The loader opens the collection root as a filesystem capability and resolves
every source below that handle. Intermediate links cannot escape the capability
and the final source component is opened with no-follow semantics, closing the
metadata-to-open replacement race as well as ordinary link traversal.

Every entry fingerprint is a versioned SHA-256 contract over its ID, relative
path, source format, Markdown policy, and normalized source. Changing content,
identity, format, or policy changes the fingerprint; newline convention does
not.

```rust
let before = articles.snapshot();
let after = CollectionSpec::new("content/articles").load::<Article>()?.snapshot();
let delta = before.diff(&after);

for id in delta.changed() {
    println!("changed: {id}");
}
# Ok::<(), pliego_content::LoadError>(())
```

`SnapshotDiff` returns sorted `added`, `changed`, and `removed` IDs. It is an
immutable input for the future build graph; this crate does not write caches or
outputs.

## Diagnostics

A failed load returns all discoverable entry problems, not only the first.
Every `ContentDiagnostic` has a stable machine code, source path, optional
portable relative path, parser format, optional byte/line/column span, and
actionable message. Current families cover I/O, roots, symlinks/reparse points,
unsupported files, paths, resource ceilings, UTF-8/BOM, frontmatter, schema deserialization,
collisions, and unsafe Markdown.

The package does not import CLI diagnostic types. A future transport layer can
serialize these values for terminal JSON without reversing the dependency.

## Reference-corpus gate

The maintained neutral fixture at `fixtures/content/reference` exercises three
independent schemas:

| Collection | Entries |
| --- | ---: |
| houses | 2 |
| journal | 2 |
| rituals | 2 |

All six YAML-compatible Markdown entries load into three different strict Rust
schemas. Two loads produce identical ID order and this fixture-contract digest:

```text
95e1ea69cc8267bfeab7fc77b0c8d5bcdd420cba4a7f14f2e7cb1f55f3d89e52
```

The package gate includes 7 unit tests, 15 integration tests, one doctest,
Clippy with warnings denied, and rustdoc with warnings denied on Rust 1.85.
`serde-saphyr` is pinned to `0.0.11`: later inspected releases require language
or standard-library features newer than the declared PliegoRS MSRV.

## Executable proof

`examples/content-collections-pliegors` builds the corpus into authored routes
through separate content, Markdown-renderer, component, route, and build
modules. It emits no generated Rust page source. CI builds it twice and requires
the ledger hash to remain identical.

Two consecutive `pliego build` passes produced the same ledger. Run
`pliego preview` inside the reference package to inspect the generated output
on an ephemeral local address.

## Deliberate limits

The package does not cache, watch, resolve asset references, or invalidate
individual outputs. Dependency tracking and selective rebuilds remain `AU-04`.
The loader is build-time native code and is not part of the browser WASM target.
