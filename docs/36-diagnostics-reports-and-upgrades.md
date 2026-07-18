# Diagnostics, reproduction reports, and upgrade checks

PliegoRS trust commands are local and read-only. They do not upload telemetry,
edit project files, resolve a mutable release channel, or repair a lockfile.

## Diagnose an environment

```sh
pliego doctor
pliego doctor --format json
```

`doctor` works outside a project and adds project checks when it finds the
nearest `pliego.toml`. It checks the CLI identity, Rust/Cargo availability,
strict manifest schema, generated output path, lockfile, first-party version
alignment, and required WASM tools. Each check has a stable `PLG-DOC-*` ID,
status, cause, and action.

Exit `0` means every required check passed. Exit `1` means a required check
failed. Warnings do not change the exit code. Malformed arguments use exit `2`.
The JSON schema is versioned by `reportVersion`.

## Create a reproduction bundle

```sh
pliego report --bundle
pliego report --bundle --output ./issue-123.tar
```

The command creates a deterministic uncompressed tar archive. It refuses to
overwrite an existing path and publishes through a temporary file in the same
directory. The bundle contains:

- `MANIFEST.json`, with the byte size and SHA-256 of every payload;
- a JSON doctor report;
- the strict `pliego.toml` manifest;
- the Cargo lock digest and a path-free first-party dependency summary;
- a build report only when it is present, valid JSON, bounded, and free of
  private absolute paths or secret markers; and
- an explicit omission ledger.

Source, content, `.env*`, VCS data, environment values, credentials, generated
binaries, and dependency caches are never traversed. The command rejects unsafe
manifest or report payloads instead of attempting to guess a replacement. It
does not upload the archive; inspect it before attaching it to an issue.

## Check an upgrade

```sh
pliego upgrade --check
pliego upgrade --check --target 0.0.1 --format json
```

The default target is the running CLI version. The command validates the strict
manifest and reads `Cargo.lock`, then reports one state:

- `compatible`: every recognized `pliego-*` package matches the target;
- `migration-required`: the running target CLI is authoritative but locked
  package versions differ; or
- `blocked`: the target CLI is not running, the lockfile is missing/invalid, or
  no recognized first-party package exists.

The check compares manifest and lockfile bytes before returning and fails if an
unexpected mutation occurred. It never writes an upgrade. Installing a target
CLI and applying migration guidance are separate, deliberate actions.

