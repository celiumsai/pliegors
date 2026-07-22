# Full-stack PliegoRS G2 reference

This application is the executable conformance fixture for the G2 full-stack
contracts published in `0.2.0-beta.1`. It is intentionally small and has no
JavaScript shell.

It proves:

- progressive login and mutation forms;
- opaque server-side sessions and login rotation;
- session-bound, action-bound CSRF;
- typed loaders and capability-scoped resources;
- an authored `404` backed by a typed loader-failure receipt;
- idempotent mutations with explicit commit state;
- public and private cache partitions;
- causal invalidation across two in-process native replicas; and
- redacted request, data, cache, action, and invalidation receipts.

The credential check and in-memory stores are conformance adapters. They are
not production identity, durability, or distributed-cache implementations.

Run one interactive replica:

```bash
cargo run -p fullstack-pliego
```

The interactive server derives its same-origin action policy from the listen
address. Set `PLIEGO_ORIGIN=https://app.example.com` when evaluating it behind
a proxy or through a different browser origin.

Run the two-replica acceptance corpus:

```bash
cargo test -p fullstack-pliego
```

Export and inspect the sealed runtime contract:

```bash
cargo run -p fullstack-pliego -- contract .pliego/runtime-contract.json
pliego inspect action rename-account
```
