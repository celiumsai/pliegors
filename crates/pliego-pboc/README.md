# pliego-pboc

`pliego-pboc` owns the Pliego Build Output Contract (PBOC): a strict,
provider-neutral manifest that binds deployable bytes, the sealed route graph,
runtime semantics, capabilities, secret references, telemetry hooks, and
deployment compatibility into one verifiable artifact.

The crate validates a manifest before any provider upload, negotiates an exact
host target, verifies the complete bundle from disk, and produces a bounded
admission receipt. It never stores provider credentials or secret values.

The current contract is `dev.pliegors.pboc/v1alpha1`. It remains preview until
the public provider TCK proves the same corpus on native/OCI and Cloudflare.
