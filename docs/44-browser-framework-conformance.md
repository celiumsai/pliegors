# Browser framework conformance

**Status:** executable preview evidence

React, Svelte, and Lit remain native ecosystem runtimes. PliegoRS does not
reimplement them. The conformance fixture bundles their normal browser packages
behind Custom Elements and runs them through adapter API v1.

Every browser extension manifest declares `entry.customElement`; the exported
`pliegoComponent.tagName`, API version, export list, and capabilities must match
that admitted descriptor before registration.

```powershell
npm run check:opensdk:browser-frameworks
```

The headless-Chrome gate proves for all three wrappers:

- mount and live property update;
- `prefers-reduced-motion` policy propagated by PliegoRS;
- HMR disposal followed by a fresh module import and remount;
- zero active framework roots and empty adapter scopes after runtime destroy;
- listener cleanup by verifying an HMR event after destroy has no effect.

The same browser run mounts an adversarial adapter that owns a real interval,
document listener, abort scope, and `MessageChannel`. HMR must replace those
resources without duplication. Runtime destroy must reduce all four counters
to zero, and later events/time must leave the counter stream unchanged.

The fixtures are examples and conformance inputs, not an additional PliegoRS
rendering abstraction. Their framework APIs remain visible and debuggable.
