import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import {
  aggregateSegments,
  allowProbeNonce,
  attachReceipt,
  injectProbe,
  percentile,
  rewriteLocation,
  summarizeResourceLedger,
  upstreamUrlForRequest,
  validateInitialRouteLedger,
  validateSegment,
  validateSessionInput,
} from "./core.mjs";

const plan = {
  devices: [
    {
      id: "phone",
      tiers: ["universal", "lite"],
      networkProfile: "mobile-4g",
    },
  ],
  targets: [
    {
      id: "work",
      routes: ["/", "/detail"],
    },
  ],
};
const fingerprints = [{ fileName: "phone.json", deviceId: "phone" }];
const sealedSourceRevision = `html-sha256:${"a".repeat(64)};manifest-sha256:${"b".repeat(64)}`;
const runSchema = JSON.parse(
  readFileSync(new URL("../../schemas/pliego.measurement-run.schema.json", import.meta.url), "utf8"),
);
const ajv = new Ajv2020({ allErrors: true, allowUnionTypes: true, strict: true });
addFormats(ajv);
const validateRun = ajv.compile(runSchema);

test("session input binds a fingerprint, canonical route, and available tier", () => {
  const result = validateSessionInput(
    {
      deviceId: "phone",
      hardwareFingerprint: "phone.json",
      targetId: "work",
      route: "/detail",
      tier: "lite",
      cacheMode: "cold",
      motionMode: "default",
      orientation: "portrait",
      networkProfile: "mobile-4g",
      powerState: "battery-over-50",
      thermalState: "nominal",
    },
    plan,
    fingerprints,
  );
  assert.deepEqual(result.errors, []);
  assert.equal(result.value.route, "/detail");
});

test("session input rejects an invented route and unsupported tier", () => {
  const result = validateSessionInput(
    {
      deviceId: "phone",
      hardwareFingerprint: "phone.json",
      targetId: "work",
      route: "/invented",
      tier: "signature",
      cacheMode: "cold",
      motionMode: "default",
      orientation: "portrait",
      networkProfile: "mobile-4g",
      powerState: "battery-over-50",
      thermalState: "nominal",
    },
    plan,
    fingerprints,
  );
  assert.match(result.errors.join("\n"), /route is not canonical/);
  assert.match(result.errors.join("\n"), /tier is not available/);
});

test("session input rejects a network profile that is not locked by the device plan", () => {
  const result = validateSessionInput(
    {
      deviceId: "phone",
      hardwareFingerprint: "phone.json",
      targetId: "work",
      route: "/detail",
      tier: "lite",
      cacheMode: "cold",
      motionMode: "default",
      orientation: "portrait",
      networkProfile: "lan-wifi",
      powerState: "battery-over-50",
      thermalState: "nominal",
    },
    plan,
    fingerprints,
  );
  assert.match(result.errors.join("\n"), /networkProfile does not match deviceId/);
});

test("segments aggregate without replacing unavailable scene metrics with invented values", () => {
  const session = {
    id: "0123456789abcdef01234567",
    deviceId: "phone",
    hardwareFingerprint: "phone.json",
    targetId: "work",
    route: "/",
    tier: "lite",
    cacheMode: "cold",
    motionMode: "default",
    orientation: "portrait",
    networkProfile: "mobile-4g",
    powerState: "battery-over-50",
    thermalState: "nominal",
    sourceRevision: sealedSourceRevision,
    cacheControl: "clear-site-data-requested",
  };
  const run = aggregateSegments(
    session,
    [segment({ final: true })],
    "2026-07-11T12:00:00.000Z",
  );
  assert.equal(run.observations.transferBytes, 1500);
  assert.equal(run.observations.sessionTransferBytes, 1500);
  assert.equal(run.observations.sessionEncodedBodyBytes, 1200);
  assert.equal(run.observations.sessionDecodedBodyBytes, 3400);
  assert.equal(run.observations.sessionResourceCount, 8);
  assert.equal(run.observations.cachedResponseCount, 0);
  assert.equal(run.initialRouteResources.length, 9);
  assert.deepEqual(validateInitialRouteLedger(run), []);
  assert.equal(run.observations.frameP95Ms, 33.4);
  assert.equal(run.observations.drawCalls, 0);
  assert.equal(run.observations.triangles, 0);
  assert.equal(run.observations.decodeP95Ms, 6.2);
  assert.equal(run.observations.postLoadDecodeP95Ms, 2.8);
  assert.equal(run.observations.mainThreadP95Ms, 18);
  assert.equal(run.observations.longTaskP95Ms, 0);
  assert.equal(run.availability.scene, "static-zero");
  assert.deepEqual(run.violations, []);

  const receipt = attachReceipt(run, "2026-07-11T12:00:01.000Z");
  assert.match(receipt.server.sha256, /^[a-f0-9]{64}$/);
  assert.equal(validateRun(receipt), true, JSON.stringify(validateRun.errors, null, 2));
});

test("WebGL without the scene hook stays explicitly unavailable", () => {
  const value = segment({ final: true });
  value.metrics.hasWebglCanvas = true;
  const run = aggregateSegments(
    {
      id: "0123456789abcdef01234567",
      deviceId: "phone",
      hardwareFingerprint: "phone.json",
      targetId: "work",
      route: "/",
      tier: "lite",
      cacheMode: "warm",
      motionMode: "default",
      orientation: "portrait",
      networkProfile: "mobile-4g",
      powerState: "battery-over-50",
      thermalState: "nominal",
      sourceRevision: sealedSourceRevision,
    },
    [value],
  );
  assert.equal(run.availability.scene, "unavailable");
  assert.equal(run.observations.drawCalls, null);
  assert.equal(run.observations.triangles, null);
});

test("scene hook VRAM and renderer counters replace DOM estimates", () => {
  const value = segment({ final: true });
  value.metrics.sceneHook = true;
  value.metrics.hasWebglCanvas = true;
  value.steps.sceneHold = "complete";
  value.metrics.estimatedVramBytes = 777_000;
  value.metrics.drawCalls = 4;
  value.metrics.triangles = 1200;
  value.metrics.drawCallSamples = [2, 3, 4];
  value.metrics.triangleSamples = [900, 1000, 1200];
  const run = aggregateSegments(
    {
      id: "0123456789abcdef01234567",
      deviceId: "phone",
      hardwareFingerprint: "phone.json",
      targetId: "work",
      route: "/",
      tier: "lite",
      cacheMode: "warm",
      motionMode: "default",
      orientation: "portrait",
      networkProfile: "mobile-4g",
      powerState: "battery-over-50",
      thermalState: "nominal",
      sourceRevision: sealedSourceRevision,
    },
    [value],
  );
  assert.equal(run.availability.scene, "pliego-scene-hook");
  assert.equal(run.observations.estimatedVramBytes, 777_000);
  assert.equal(run.observations.drawCalls, 4);
  assert.equal(run.observations.triangles, 1200);
});

test("scene hold state must agree with observed scene presence", () => {
  const staticValue = segment({ final: true });
  staticValue.steps.sceneHold = "complete";
  const staticRun = aggregateSegments(baseSession("warm"), [staticValue]);
  assert.ok(staticRun.violations.includes("scene-hold-complete-without-scene"));

  const canvasValue = segment({ final: true });
  canvasValue.metrics.hasWebglCanvas = true;
  canvasValue.steps.sceneHold = "not-applicable";
  const canvasRun = aggregateSegments(baseSession("warm"), [canvasValue]);
  assert.ok(canvasRun.violations.includes("scene-hold-not-applicable-with-scene"));
});

test("selected orientation, viewport, and renderer must match observed evidence", () => {
  const run = aggregateSegments(
    {
      id: "0123456789abcdef01234567",
      deviceId: "phone",
      hardwareFingerprint: "phone.json",
      targetId: "work",
      route: "/",
      tier: "lite",
      cacheMode: "warm",
      motionMode: "default",
      orientation: "landscape",
      networkProfile: "mobile-4g",
      powerState: "battery-over-50",
      thermalState: "nominal",
      sourceRevision: sealedSourceRevision,
      expectedRenderer: "Expected GPU",
      expectedViewport: {
        mode: "fixed",
        orientation: "landscape",
        width: 1440,
        height: 900,
        deviceScaleFactor: 1,
      },
    },
    [segment({ final: true })],
  );
  assert.ok(run.violations.includes("orientation-mismatch:portrait"));
  assert.ok(run.violations.some((value) => value.startsWith("viewport-width-mismatch")));
  assert.ok(run.violations.includes("renderer-fingerprint-mismatch"));
  assert.equal(run.viewport.orientation, "portrait");
});

test("native viewport accepts dynamic browser chrome heights within the fingerprint range", () => {
  const session = {
    ...baseSession("warm"),
    expectedViewport: {
      mode: "native",
      orientation: "portrait",
      width: 384,
      height: 732,
      deviceScaleFactor: 2.8125,
    },
    expectedFingerprintViewport: {
      innerWidth: 384,
      innerHeight: 732,
      visualWidth: 384,
      visualHeight: 732.8,
      availableWidth: 384,
      availableHeight: 790,
      devicePixelRatio: 2.8125,
      orientation: "portrait-primary",
    },
  };

  for (const height of [732, 789]) {
    const value = segment({ final: true });
    value.viewport.height = height;
    value.viewport.devicePixelRatio = 2.8125;
    const run = aggregateSegments(session, [value]);
    assert.equal(
      run.violations.some((violation) => violation.startsWith("viewport-height-mismatch")),
      false,
      `${height}px should be accepted`,
    );
  }

  const outside = segment({ final: true });
  outside.viewport.height = 797;
  outside.viewport.devicePixelRatio = 2.8125;
  const rejected = aggregateSegments(session, [outside]);
  assert.ok(rejected.violations.includes("viewport-height-mismatch:797"));
});

test("fixed viewport height remains exact instead of using the fingerprint range", () => {
  const value = segment({ final: true });
  value.viewport.height = 789;
  value.viewport.devicePixelRatio = 2.8125;
  const run = aggregateSegments(
    {
      ...baseSession("warm"),
      expectedViewport: {
        mode: "fixed",
        orientation: "portrait",
        width: 384,
        height: 732,
        deviceScaleFactor: 2.8125,
      },
      expectedFingerprintViewport: {
        innerHeight: 732,
        visualHeight: 732.8,
        availableHeight: 790,
        devicePixelRatio: 2.8125,
        orientation: "portrait-primary",
      },
    },
    [value],
  );
  assert.ok(run.violations.includes("viewport-height-mismatch:789"));
});

test("first-viewport snapshot stays separate from complete document totals", () => {
  const first = segment();
  first.metrics.transferBytes = 2100;
  first.metrics.encodedBodyBytes = 1800;
  first.metrics.decodedBodyBytes = 4000;
  first.metrics.resourceCount = 10;
  const second = segment({ final: true, pagePath: "/detail" });
  second.segmentId = "fedcba9876543210";
  second.metrics.transferBytes = 900;
  second.metrics.resourceCount = 3;
  const run = aggregateSegments(
    {
      id: "0123456789abcdef01234567",
      deviceId: "phone",
      hardwareFingerprint: "phone.json",
      targetId: "work",
      route: "/",
      tier: "lite",
      cacheMode: "cold",
      motionMode: "default",
      orientation: "portrait",
      networkProfile: "mobile-4g",
      powerState: "battery-over-50",
      thermalState: "nominal",
      sourceRevision: sealedSourceRevision,
    },
    [first, second],
  );
  assert.equal(run.observations.transferBytes, 1500);
  assert.equal(run.observations.sessionTransferBytes, 3000);
  assert.equal(run.observations.sessionEncodedBodyBytes, 3000);
  assert.equal(run.observations.sessionDecodedBodyBytes, 7400);
  assert.equal(run.observations.resourceCount, 8);
  assert.equal(run.observations.sessionResourceCount, 13);
});

test("the first canonical snapshot wins over a later return to the same route", () => {
  const first = segment({ capturedAt: "2026-07-11T12:00:00.000Z" });
  const detail = segment({
    capturedAt: "2026-07-11T12:00:10.000Z",
    pagePath: "/detail",
  });
  detail.segmentId = "1111111111111111";
  const returned = segment({
    capturedAt: "2026-07-11T12:00:20.000Z",
    final: true,
    initialSnapshot: measurementSnapshot("/", 1),
  });
  returned.segmentId = "2222222222222222";
  const run = aggregateSegments(baseSession("warm"), [returned, detail, first]);
  assert.equal(run.observations.transferBytes, 1500);
  assert.equal(run.observations.resourceCount, 8);
  assert.equal(run.initialRouteResources.length, 9);
});

test("diagnostic probes do not impersonate missing acceptance metrics", () => {
  const value = segment({ final: true });
  value.metrics.activeTier = null;
  value.metrics.targetDecodeDurations = [];
  value.metrics.targetMainThreadDurations = [];
  const run = aggregateSegments(
    {
      id: "0123456789abcdef01234567",
      deviceId: "phone",
      hardwareFingerprint: "phone.json#sha256=abc",
      targetId: "work",
      route: "/",
      tier: "lite",
      cacheMode: "warm",
      motionMode: "default",
      orientation: "portrait",
      networkProfile: "mobile-4g",
      powerState: "battery-over-50",
      thermalState: "nominal",
      sourceRevision: sealedSourceRevision,
    },
    [value],
  );
  assert.equal(run.observations.decodeP95Ms, null);
  assert.equal(run.observations.postLoadDecodeP95Ms, 2.8);
  assert.equal(run.observations.mainThreadP95Ms, null);
  assert.equal(run.availability.decode, "post-load-probe-only");
  assert.equal(run.availability.mainThread, "longtask-probe-only");
  assert.ok(run.violations.includes("tier-selection-unverified"));
});

test("segment validation rejects a foreign session", () => {
  const errors = validateSegment(segment(), "ffffffffffffffffffffffff");
  assert.match(errors.join("\n"), /sessionId does not match/);
});

test("segment validation rejects snapshot totals that do not reconcile with the ledger", () => {
  const value = segment();
  value.initialSnapshot.transferBytes += 1;
  const errors = validateSegment(value, value.sessionId);
  assert.match(errors.join("\n"), /transferBytes does not reconcile/);
});

test("segment validation rejects a cache hit mislabeled as a network response", () => {
  const value = segment();
  const cachedResource = value.initialSnapshot.resources[1];
  cachedResource.transferBytes = 0;
  cachedResource.cacheState = "network";
  const { navigationCount: _navigationCount, ...summary } = summarizeResourceLedger(
    value.initialSnapshot.resources,
  );
  Object.assign(value.initialSnapshot, summary);

  const errors = validateSegment(value, value.sessionId);
  assert.match(errors.join("\n"), /cacheState is network, expected local-cache/);
});

test("run ledger validation catches a changed entry even when a receipt can be regenerated", () => {
  const run = aggregateSegments(
    {
      id: "0123456789abcdef01234567",
      deviceId: "phone",
      hardwareFingerprint: "phone.json",
      targetId: "work",
      route: "/",
      tier: "lite",
      cacheMode: "warm",
      motionMode: "default",
      orientation: "portrait",
      networkProfile: "mobile-4g",
      powerState: "battery-over-50",
      thermalState: "nominal",
      sourceRevision: sealedSourceRevision,
    },
    [segment({ final: true })],
  );
  const originalReceipt = attachReceipt(run, "2026-07-11T12:00:01.000Z");
  run.initialRouteResources[0].transferBytes += 1;
  const resealed = attachReceipt(run, "2026-07-11T12:00:01.000Z");
  assert.match(resealed.server.sha256, /^[a-f0-9]{64}$/);
  assert.notEqual(resealed.server.sha256, originalReceipt.server.sha256);
  assert.match(validateInitialRouteLedger(resealed).join("\n"), /transferBytes is/);
});

test("opaque, unknown, and overflowed first-viewport ledgers become run violations", () => {
  const opaque = segment({ final: true });
  opaque.initialSnapshot.resources[1].cacheState = "opaque";
  const opaqueRun = aggregateSegments(baseSession("warm"), [opaque]);
  assert.ok(opaqueRun.violations.includes("initial-resource-cache-opaque"));

  const unknown = segment({ final: true });
  unknown.initialSnapshot.resources[1].cacheState = "unknown";
  const unknownRun = aggregateSegments(baseSession("warm"), [unknown]);
  assert.ok(unknownRun.violations.includes("initial-resource-cache-unknown"));

  const overflow = segment({
    final: true,
    initialSnapshot: measurementSnapshot("/", 511, true),
  });
  assert.deepEqual(validateSegment(overflow, overflow.sessionId), []);
  const overflowRun = aggregateSegments(baseSession("warm"), [overflow]);
  assert.ok(overflowRun.violations.includes("initial-resource-ledger-overflow"));
});

test("probe injection is idempotent and precedes authored head content", () => {
  const html = "<!doctype html><html><head><script src=\"/app.js\"></script></head></html>";
  const once = injectProbe(html, "", { tier: "lite", motionMode: "reduced" });
  const twice = injectProbe(once, "", { tier: "lite", motionMode: "reduced" });
  assert.equal(once, twice);
  assert.ok(
    once.indexOf("__PLIEGO_REQUESTED_TIER__") <
      once.indexOf("/_pliego/probe-contract.js"),
  );
  assert.ok(once.indexOf("/_pliego/probe-contract.js") < once.indexOf("/_pliego/probe.js"));
  assert.ok(once.indexOf("/_pliego/probe.js") < once.indexOf("/app.js"));
  assert.match(once, /__PLIEGO_REQUESTED_TIER__="lite"/);
  assert.match(once, /__PLIEGO_REQUESTED_MOTION__="reduced"/);
});

test("a strict CSP receives only the probe nonce needed by the injected script", () => {
  const nonce = "abc123";
  const html = injectProbe("<html><head></head></html>", nonce);
  const csp = allowProbeNonce("default-src 'none'; script-src 'strict-dynamic';", nonce);
  assert.match(html, /nonce="abc123"/);
  assert.equal(
    csp,
    "default-src 'none'; script-src 'strict-dynamic' 'nonce-abc123';",
  );
});

test("CSP nonce injection preserves multiple policies and directive casing", () => {
  const csp = allowProbeNonce(
    "DEFAULT-SRC 'none'; SCRIPT-SRC-ELEM 'self', default-src 'self'",
    "run456",
  );
  assert.equal(
    csp,
    "DEFAULT-SRC 'none'; SCRIPT-SRC-ELEM 'self' 'nonce-run456';, " +
      "default-src 'self'; script-src 'nonce-run456';",
  );
  assert.deepEqual(
    allowProbeNonce(["script-src 'self'", "default-src 'none'"], "run456"),
    [
      "script-src 'self' 'nonce-run456';",
      "default-src 'none'; script-src 'nonce-run456';",
    ],
  );
});

test("same-upstream redirects remain on the measurement origin", () => {
  assert.equal(
    rewriteLocation("http://127.0.0.1:4200/pliego?x=1", "http://127.0.0.1:4200"),
    "/pliego?x=1",
  );
  assert.equal(
    rewriteLocation("https://example.com/out", "http://127.0.0.1:4200"),
    "https://example.com/out",
  );
});

test("absolute-form request targets cannot replace the allowlisted upstream", () => {
  const value = upstreamUrlForRequest(
    "http://169.254.169.254/latest/meta-data?secret=1",
    "http://127.0.0.1:4200",
  );
  assert.equal(value.origin, "http://127.0.0.1:4200");
  assert.equal(value.pathname, "/latest/meta-data");
  assert.equal(value.search, "?secret=1");
});

test("percentile uses the nearest-rank contract", () => {
  assert.equal(percentile([16.6, 16.7, 33.4, 16.8], 0.95), 33.4);
});

function segment(overrides = {}) {
  const pagePath = overrides.pagePath ?? "/";
  return {
    segmentVersion: "1.1.0",
    capturedAt: overrides.capturedAt ?? "2026-07-11T11:59:59.000Z",
    sessionId: "0123456789abcdef01234567",
    segmentId: "0123456789abcdef",
    final: overrides.final ?? false,
    pagePath,
    durationMs: 10_500,
    browserFingerprint: "Browser/1.0",
    viewport: {
      width: 384,
      height: 732,
      devicePixelRatio: 2.8,
    },
    initialSnapshot: overrides.initialSnapshot ?? measurementSnapshot(pagePath),
    conditions: {
      foreground: true,
      reducedMotionMatched: false,
      serviceWorkerControlled: false,
    },
    steps: {
      ready: "complete",
      scroll: "complete",
      navigation: "not-applicable",
      visualResponse: "complete",
      disclosure: "not-applicable",
      sceneHold: "not-applicable",
      returnToInitial: "not-applicable",
    },
    metrics: {
      transferBytes: 1500,
      encodedBodyBytes: 1200,
      decodedBodyBytes: 3400,
      resourceCount: 8,
      cachedResponseCount: 0,
      decodeDurations: [1.2, 2.8],
      targetDecodeDurations: [4.1, 6.2],
      longTaskDurations: [],
      targetMainThreadDurations: [8, 18],
      interactionDurations: [32],
      estimatedVramBytes: 8192,
      drawCalls: null,
      triangles: null,
      drawCallSamples: [],
      triangleSamples: [],
      sceneHook: false,
      activeTier: "lite",
      webglRenderer: "Test GPU",
      hasWebglCanvas: false,
      frameDeltas: [16.6, 16.7, 16.8, 33.4],
      lcpMs: 811,
      cls: 0.01,
    },
    capabilities: {
      lcp: true,
      longtask: true,
      eventTiming: true,
      layoutShift: true,
    },
    violations: [],
  };
}

function measurementSnapshot(path = "/", resourceCount = 8, overflowed = false) {
  const resources = [
    {
      entryType: "navigation",
      scope: "target-origin",
      path,
      initiator: "navigation",
      transferBytes: 300,
      encodedBodyBytes: 200,
      decodedBodyBytes: 400,
      cacheState: "network",
      durationMs: 40,
    },
    ...Array.from({ length: resourceCount }, (_, index) => ({
      entryType: "resource",
      scope: "target-origin",
      path: `/assets/resource-${index}.js`,
      initiator: "script",
      transferBytes: 150,
      encodedBodyBytes: 125,
      decodedBodyBytes: 375,
      cacheState: "network",
      durationMs: 12 + index / 100,
    })),
  ];
  const { navigationCount: _navigationCount, ...summary } =
    summarizeResourceLedger(resources);
  return {
    capturedAt: "2026-07-11T11:59:58.000Z",
    overflowed,
    resources,
    ...summary,
  };
}

function baseSession(cacheMode) {
  return {
    id: "0123456789abcdef01234567",
    deviceId: "phone",
    hardwareFingerprint: "phone.json",
    targetId: "work",
    route: "/",
    tier: "lite",
    cacheMode,
    motionMode: "default",
    orientation: "portrait",
    networkProfile: "mobile-4g",
    powerState: "battery-over-50",
    thermalState: "nominal",
    sourceRevision: sealedSourceRevision,
  };
}
