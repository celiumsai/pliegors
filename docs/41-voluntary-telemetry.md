# Voluntary telemetry

**Status:** implemented locally; no network collector exists  
**Policy version:** `1.0.0`

PliegoRS telemetry is disabled by default. An absent telemetry state is the
disabled state, and normal installation, scaffolding, checking, development,
and building do not create telemetry files or make telemetry network requests.
No environment variable, installer flag, project manifest, or Pliego.run
account can silently enable it.

## Consent and controls

Consent requires this explicit command:

```sh
pliego telemetry enable
```

The command records `install` as the moment the already-installed CLI enters
the voluntary funnel. It does not claim to know when crates.io or a platform
installer transferred the binary. Successful later commands may record
`new`, `check`, `dev`, and `build` while consent remains enabled.

The complete user controls are:

```sh
pliego telemetry status
pliego telemetry status --format json
pliego telemetry preview
pliego telemetry preview --format json
pliego telemetry export --output ./pliegors-telemetry.json
pliego telemetry disable
pliego telemetry disable --delete-local
```

`preview` shows the exact report shape. `export` creates a new local file and
refuses to overwrite an existing path. Neither command uploads data. Disabling
stops collection; `--delete-local` also removes the telemetry state. Re-enabling
requires another deliberate command.

## Exact allowlist

Each event contains exactly:

- monotonic local sequence;
- one of `install`, `new`, `check`, `dev`, or `build`;
- day since the Unix epoch, deliberately coarser than a timestamp;
- PliegoRS CLI version;
- operating-system platform; and
- CPU architecture.

The report contains the policy version, consent state, generation day, explicit
field allowlist, and at most 64 events. Oldest events are discarded when that
bound is reached. It contains no installation or user identifier, IP address,
path, project name, template, route, arguments, source, environment value,
error, diagnostic, dependency list, hostname, username, or email address.

Local state lives below `${PLIEGO_HOME}/telemetry` or the platform user's
`.pliego/telemetry` directory. It uses a bounded, exact JSON contract, refuses
links and unknown fields, serializes concurrent writers through a local lock,
and uses owner-only modes on Unix. A corrupt or unsupported state fails closed:
it cannot enable collection. Telemetry recording never changes the success or
failure of `new`, `check`, `dev`, or `build`.

The export schema is
[`pliego.telemetry-report.schema.json`](../schemas/pliego.telemetry-report.schema.json).

## Network boundary

There is no built-in submission command, retry queue, collector URL, API key,
cookie, or background process in P8. `networkSubmission` is therefore `none`.
This makes the default network-denial property structural rather than a toggle
that a remote service must honor.

A future collector must be proposed separately. It may not activate existing
clients remotely or expand this schema in place. It requires a new policy
version, local preview of the exact payload, a deliberate submission action,
bounded retries, redaction/adversarial tests, published retention and deletion
terms, and independent evidence that disabled mode sends no request. Until
then, a developer may inspect and share an exported report manually.

## Acceptance boundary

Local tests prove absent-state behavior, explicit enablement, the 64-event
bound, schema validation, unknown-field rejection, no-proxy connection during
control commands, export non-overwrite, and deletion. The release-only golden
matrix verifies telemetry is disabled with zero events before and after the
first-use path.

Those tests do not prove adoption, a submission rate, or a hosted data service.
PliegoRS publishes no funnel metric until voluntarily supplied reports exist
and their collection basis is disclosed.
