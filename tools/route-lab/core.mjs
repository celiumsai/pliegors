import { createHash } from "node:crypto";

export const COLLECTOR_VERSION = "pliego-route-lab/0.2.2";
export const RUN_SCHEMA = "https://pliegors.dev/schemas/pliego.measurement-run.schema.json";

const RESOURCE_LEDGER_LIMIT = 512;
const resourceEntryTypes = new Set(["navigation", "resource"]);
const resourceScopes = new Set(["target-origin", "external"]);
const cacheStates = new Set([
  "network",
  "local-cache",
  "validated-cache",
  "opaque",
  "unknown",
]);

const tiers = new Set(["universal", "lite", "balanced", "signature"]);
const cacheModes = new Set(["cold", "warm"]);
const motionModes = new Set(["default", "reduced"]);
const orientations = new Set(["portrait", "landscape"]);
const powerStates = new Set(["external-power", "battery-over-50", "unknown"]);
const thermalStates = new Set(["nominal", "throttled", "unknown"]);

export function validateSessionInput(input, plan, fingerprints) {
  const errors = [];
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return { errors: ["body must be an object"] };
  }

  const device = plan.devices.find((candidate) => candidate.id === input.deviceId);
  const target = plan.targets.find((candidate) => candidate.id === input.targetId);
  const fingerprint = fingerprints.find(
    (candidate) => candidate.fileName === input.hardwareFingerprint,
  );

  if (!device) errors.push("deviceId is not in the measurement plan");
  if (!target) errors.push("targetId is not in the measurement plan");
  if (!fingerprint || fingerprint.deviceId !== input.deviceId) {
    errors.push("hardwareFingerprint does not belong to deviceId");
  }
  if (target && !target.routes.includes(input.route)) {
    errors.push("route is not canonical for targetId");
  }
  if (!tiers.has(input.tier) || (device && !device.tiers.includes(input.tier))) {
    errors.push("tier is not available for deviceId");
  }
  if (!cacheModes.has(input.cacheMode)) errors.push("cacheMode is invalid");
  if (!motionModes.has(input.motionMode)) errors.push("motionMode is invalid");
  if (!orientations.has(input.orientation)) errors.push("orientation is invalid");
  if (!powerStates.has(input.powerState)) errors.push("powerState is invalid");
  if (!thermalStates.has(input.thermalState)) errors.push("thermalState is invalid");
  if (typeof input.networkProfile !== "string" || !input.networkProfile.trim()) {
    errors.push("networkProfile is required");
  } else if (device && input.networkProfile.trim() !== device.networkProfile) {
    errors.push("networkProfile does not match deviceId");
  }

  if (errors.length) return { errors };
  return {
    errors,
    value: {
      deviceId: input.deviceId,
      hardwareFingerprint: input.hardwareFingerprint,
      targetId: input.targetId,
      route: input.route,
      tier: input.tier,
      cacheMode: input.cacheMode,
      motionMode: input.motionMode,
      orientation: input.orientation,
      networkProfile: input.networkProfile.trim(),
      powerState: input.powerState,
      thermalState: input.thermalState,
    },
  };
}

export function validateSegment(segment, sessionOrId) {
  const session =
    typeof sessionOrId === "string" ? { id: sessionOrId } : (sessionOrId ?? {});
  const errors = [];
  if (!segment || typeof segment !== "object" || Array.isArray(segment)) {
    return ["body must be an object"];
  }
  if (segment.segmentVersion !== "1.1.0") errors.push("segmentVersion must be 1.1.0");
  if (segment.sessionId !== session.id) errors.push("sessionId does not match the cookie");
  if (!/^[a-f0-9]{16}$/.test(segment.segmentId ?? "")) {
    errors.push("segmentId is invalid");
  }
  if (!segment.browserFingerprint) errors.push("browserFingerprint is required");
  if (
    session.expectedUserAgent &&
    segment.browserFingerprint !== session.expectedUserAgent
  ) {
    errors.push("browserFingerprint does not match the accepted device fingerprint");
  }
  if (typeof segment.final !== "boolean") errors.push("final must be a boolean");
  if (!Number.isFinite(Date.parse(segment.capturedAt))) errors.push("capturedAt is invalid");
  if (!validLedgerPath(segment.pagePath)) {
    errors.push("pagePath must be a pathname without origin, query, or fragment");
  }
  if (!within(segment.durationMs, 1, 30 * 60 * 1000)) errors.push("durationMs is invalid");
  if (!segment.viewport || !segment.metrics || !segment.capabilities) {
    errors.push("viewport, metrics, and capabilities are required");
  }
  if (
    !within(segment.viewport?.width, 1, 16_384) ||
    !within(segment.viewport?.height, 1, 16_384) ||
    !within(segment.viewport?.devicePixelRatio, 0.1, 16)
  ) {
    errors.push("viewport values are invalid");
  }
  for (const field of ["transferBytes", "encodedBodyBytes", "decodedBodyBytes", "resourceCount", "cachedResponseCount", "estimatedVramBytes"]) {
    const value = segment.metrics?.[field];
    if (!Number.isSafeInteger(value) || value < 0) errors.push(`${field} is invalid`);
  }
  for (const field of ["drawCalls", "triangles"]) {
    const value = segment.metrics?.[field];
    if (value !== null && (!Number.isSafeInteger(value) || value < 0)) {
      errors.push(`${field} is invalid`);
    }
  }
  for (const field of [
    "frameDeltas",
    "decodeDurations",
    "targetDecodeDurations",
    "longTaskDurations",
    "targetMainThreadDurations",
    "interactionDurations",
    "drawCallSamples",
    "triangleSamples",
  ]) {
    if (!validMetricArray(segment.metrics?.[field])) errors.push(`${field} is invalid`);
  }
  if (typeof segment.conditions?.foreground !== "boolean") {
    errors.push("conditions.foreground is invalid");
  }
  if (typeof segment.conditions?.reducedMotionMatched !== "boolean") {
    errors.push("conditions.reducedMotionMatched is invalid");
  }
  if (typeof segment.conditions?.serviceWorkerControlled !== "boolean") {
    errors.push("conditions.serviceWorkerControlled is invalid");
  }
  for (const field of ["lcp", "longtask", "eventTiming", "layoutShift"]) {
    if (typeof segment.capabilities?.[field] !== "boolean") {
      errors.push(`capabilities.${field} is invalid`);
    }
  }
  for (const field of ["lcpMs", "cls"]) {
    const value = segment.metrics?.[field];
    if (value !== null && !within(value, 0, 60 * 60 * 1000)) {
      errors.push(`${field} is invalid`);
    }
  }
  if (
    segment.metrics?.activeTier !== null &&
    typeof segment.metrics?.activeTier !== "string"
  ) {
    errors.push("activeTier is invalid");
  }
  if (
    segment.metrics?.webglRenderer !== null &&
    typeof segment.metrics?.webglRenderer !== "string"
  ) {
    errors.push("webglRenderer is invalid");
  }
  const stepStates = new Set(["complete", "not-applicable", "missed"]);
  if (
    !segment.steps ||
    Object.values(segment.steps).some((value) => !stepStates.has(value))
  ) {
    errors.push("steps contain an invalid state");
  }
  if (!Array.isArray(segment.violations)) errors.push("violations must be an array");
  errors.push(...validateInitialSnapshot(segment.initialSnapshot, segment.pagePath));
  return errors;
}

export function summarizeResourceLedger(entries = []) {
  const resources = Array.isArray(entries) ? entries : [];
  return {
    transferBytes: sum(resources.map((entry) => entry?.transferBytes)),
    encodedBodyBytes: sum(resources.map((entry) => entry?.encodedBodyBytes)),
    decodedBodyBytes: sum(resources.map((entry) => entry?.decodedBodyBytes)),
    resourceCount: resources.filter((entry) => entry?.entryType === "resource").length,
    cachedResponseCount: resources.filter((entry) =>
      ["local-cache", "validated-cache"].includes(entry?.cacheState),
    ).length,
    navigationCount: resources.filter((entry) => entry?.entryType === "navigation").length,
  };
}

export function cacheStateForTiming({
  scope,
  transferBytes,
  encodedBodyBytes,
  decodedBodyBytes,
}) {
  if (transferBytes > 0 && encodedBodyBytes === 0 && decodedBodyBytes > 0) {
    return "validated-cache";
  }
  if (transferBytes > 0) return "network";
  if (decodedBodyBytes > 0) return "local-cache";
  if (
    scope === "external" &&
    transferBytes === 0 &&
    encodedBodyBytes === 0 &&
    decodedBodyBytes === 0
  ) {
    return "opaque";
  }
  return "unknown";
}

export function validateInitialRouteLedger(run) {
  const errors = validateResourceLedger(run?.initialRouteResources, run?.route);
  if (errors.length) return errors;
  const summary = summarizeResourceLedger(run.initialRouteResources);
  for (const field of [
    "transferBytes",
    "encodedBodyBytes",
    "decodedBodyBytes",
    "resourceCount",
    "cachedResponseCount",
  ]) {
    if (run.observations?.[field] !== summary[field]) {
      errors.push(
        `${field} is ${run.observations?.[field]}, resource ledger recalculates ${summary[field]}`,
      );
    }
  }
  if (run.initialRouteResources.some((entry) => entry.cacheState === "opaque")) {
    errors.push("resource ledger contains opaque timing data");
  }
  if (run.initialRouteResources.some((entry) => entry.cacheState === "unknown")) {
    errors.push("resource ledger contains unknown cache state");
  }
  return errors;
}

export function upstreamUrlForRequest(requestTarget, upstreamBase) {
  const incoming = new URL(requestTarget, "http://route-lab.invalid");
  const upstream = new URL(upstreamBase);
  upstream.pathname = incoming.pathname;
  upstream.search = incoming.search;
  upstream.hash = "";
  return upstream;
}

export function aggregateSegments(session, segments, capturedAt = new Date().toISOString()) {
  const ordered = [...segments].sort(
    (left, right) => Date.parse(left.capturedAt) - Date.parse(right.capturedAt),
  );
  if (!ordered.length) throw new Error("at least one segment is required");

  const first = ordered[0];
  const final = [...ordered].reverse().find((segment) => segment.final) ?? ordered.at(-1);
  const frameDeltas = ordered.flatMap((segment) => finiteNumbers(segment.metrics.frameDeltas));
  if (!frameDeltas.length) throw new Error("the run has no frame samples");
  const decodeDurations = ordered.flatMap((segment) =>
    finiteNumbers(segment.metrics.decodeDurations),
  );
  const targetDecodeDurations = ordered.flatMap((segment) =>
    finiteNumbers(segment.metrics.targetDecodeDurations),
  );
  const longTasks = ordered.flatMap((segment) =>
    finiteNumbers(segment.metrics.longTaskDurations),
  );
  const targetMainThreadDurations = ordered.flatMap((segment) =>
    finiteNumbers(segment.metrics.targetMainThreadDurations),
  );
  const interactionDurations = ordered.flatMap((segment) =>
    finiteNumbers(segment.metrics.interactionDurations),
  );
  const initial =
    ordered.find((segment) => normalizePath(segment.pagePath) === normalizePath(session.route)) ??
    first;
  const initialRouteResources = initial.initialSnapshot.resources.map((entry) => ({
    ...entry,
  }));
  const initialResourceSummary = summarizeResourceLedger(initialRouteResources);
  const sceneHook = ordered.some((segment) => segment.metrics.sceneHook === true);
  const hasWebglCanvas = ordered.some((segment) => segment.metrics.hasWebglCanvas === true);
  const longTaskSupported = ordered.some((segment) => segment.capabilities.longtask === true);
  const lcpSupported = initial.capabilities.lcp === true;
  const inpSupported = ordered.some((segment) => segment.capabilities.eventTiming === true);
  const clsSupported = initial.capabilities.layoutShift === true;
  const steps = normalizeSteps(final.steps);
  const violations = new Set(ordered.flatMap((segment) => segment.violations ?? []));

  if (initial.initialSnapshot.overflowed) {
    violations.add("initial-resource-ledger-overflow");
  }
  if (initialRouteResources.some((entry) => entry.cacheState === "opaque")) {
    violations.add("initial-resource-cache-opaque");
  }
  if (initialRouteResources.some((entry) => entry.cacheState === "unknown")) {
    violations.add("initial-resource-cache-unknown");
  }

  if (ordered.some((segment) => segment.conditions?.foreground === false)) {
    violations.add("document-left-foreground");
  }
  if (session.motionMode === "reduced" && !final.conditions?.reducedMotionMatched) {
    violations.add("reduced-motion-not-active");
  }
  if (session.motionMode === "default" && final.conditions?.reducedMotionMatched) {
    violations.add("default-motion-not-active");
  }
  if (session.thermalState !== "nominal") violations.add("thermal-state-not-nominal");
  if (session.powerState === "unknown") violations.add("power-state-unattested");
  if (!/^html-sha256:[a-f0-9]{64};manifest-sha256:[a-f0-9]{64}$/.test(session.sourceRevision)) {
    violations.add("source-revision-unsealed");
  }
  if (ordered.some((segment) => segment.conditions?.serviceWorkerControlled)) {
    violations.add("service-worker-controlled-route");
  }
  if (!final.metrics.activeTier) {
    violations.add("tier-selection-unverified");
  } else if (final.metrics.activeTier !== session.tier) {
    violations.add(`tier-selection-mismatch:${final.metrics.activeTier}`);
  }
  const actualOrientation =
    finiteOrZero(initial.viewport.width) >= finiteOrZero(initial.viewport.height)
      ? "landscape"
      : "portrait";
  if (actualOrientation !== session.orientation) {
    violations.add(`orientation-mismatch:${actualOrientation}`);
  }
  validateExpectedViewport(session, initial.viewport, actualOrientation, violations);
  const observedRenderer =
    typeof final.metrics.webglRenderer === "string" ? final.metrics.webglRenderer : null;
  if (session.expectedRenderer && !observedRenderer) {
    violations.add("renderer-fingerprint-unavailable");
  } else if (session.expectedRenderer && observedRenderer !== session.expectedRenderer) {
    violations.add("renderer-fingerprint-mismatch");
  }
  for (const [step, state] of Object.entries(steps)) {
    if (state === "missed") violations.add(`interaction-step-missed:${step}`);
  }
  if (steps.sceneHold === "complete" && !hasWebglCanvas) {
    violations.add("scene-hold-complete-without-scene");
  }
  if (steps.sceneHold === "not-applicable" && hasWebglCanvas) {
    violations.add("scene-hold-not-applicable-with-scene");
  }

  const durationMs = Math.max(
    0,
    ordered.reduce((total, segment) => total + finiteOrZero(segment.durationMs), 0),
  );
  if (durationMs < 10_000) violations.add("interaction-window-under-10s");

  const scene = aggregateScene(ordered, sceneHook, hasWebglCanvas);
  if (sceneHook && (scene.drawCalls === null || scene.triangles === null)) {
    violations.add("scene-frame-samples-unavailable");
  }
  const initialTransferBytes = initialResourceSummary.transferBytes;
  if (session.cacheMode === "cold" && initialTransferBytes === 0) {
    violations.add("cold-cache-transfer-not-observed");
  }
  if (
    session.cacheMode === "cold" &&
    initialResourceSummary.cachedResponseCount > 0
  ) {
    violations.add("cold-cache-contained-cached-responses");
  }
  const run = {
    $schema: RUN_SCHEMA,
    runVersion: "1.1.0",
    planVersion: "1.0.0",
    capturedAt,
    sessionId: session.id,
    deviceId: session.deviceId,
    hardwareFingerprint: session.hardwareFingerprint,
    browserFingerprint: final.browserFingerprint,
    targetId: session.targetId,
    route: session.route,
    tier: session.tier,
    cacheMode: session.cacheMode,
    motionMode: session.motionMode,
    sourceRevision: session.sourceRevision,
    observedRenderer,
    viewport: {
      width: finiteOrZero(initial.viewport.width),
      height: finiteOrZero(initial.viewport.height),
      devicePixelRatio: finiteOrZero(initial.viewport.devicePixelRatio),
      orientation: actualOrientation,
    },
    networkProfile: session.networkProfile,
    initialRouteResources,
    conditions: {
      foreground: !violations.has("document-left-foreground"),
      powerState: session.powerState,
      thermalState: session.thermalState,
      cacheControl:
        session.cacheMode === "cold"
          ? session.cacheControl ?? "clear-site-data-requested"
          : "warm-cache-preserved",
      reducedMotionMatched: Boolean(final.conditions?.reducedMotionMatched),
      serviceWorkerControlled: ordered.some(
        (segment) => segment.conditions?.serviceWorkerControlled === true,
      ),
    },
    interaction: {
      durationMs: round(durationMs),
      steps,
    },
    observations: {
      transferBytes: initialTransferBytes,
      sessionTransferBytes: sum(ordered.map((segment) => segment.metrics.transferBytes)),
      encodedBodyBytes: initialResourceSummary.encodedBodyBytes,
      sessionEncodedBodyBytes: sum(
        ordered.map((segment) => segment.metrics.encodedBodyBytes),
      ),
      decodedBodyBytes: initialResourceSummary.decodedBodyBytes,
      sessionDecodedBodyBytes: sum(
        ordered.map((segment) => segment.metrics.decodedBodyBytes),
      ),
      resourceCount: initialResourceSummary.resourceCount,
      sessionResourceCount: sum(ordered.map((segment) => segment.metrics.resourceCount)),
      cachedResponseCount: initialResourceSummary.cachedResponseCount,
      sessionCachedResponseCount: sum(
        ordered.map((segment) => segment.metrics.cachedResponseCount),
      ),
      decodeP95Ms: targetDecodeDurations.length
        ? percentile(targetDecodeDurations, 0.95)
        : null,
      postLoadDecodeP95Ms: decodeDurations.length ? percentile(decodeDurations, 0.95) : null,
      mainThreadP95Ms: targetMainThreadDurations.length
        ? percentile(targetMainThreadDurations, 0.95)
        : null,
      longTaskP95Ms: longTaskSupported ? percentile(longTasks, 0.95) : null,
      estimatedVramBytes: scene.estimatedVramBytes,
      drawCalls: scene.drawCalls,
      triangles: scene.triangles,
      frameSampleCount: frameDeltas.length,
      frameP95Ms: percentile(frameDeltas, 0.95),
      framesOver33Ms: frameDeltas.filter((value) => value > 33).length,
      lcpMs: lcpSupported ? finiteOrNull(initial.metrics.lcpMs) : null,
      inpMs:
        inpSupported && interactionDurations.length
          ? round(Math.max(...interactionDurations))
          : null,
      cls: clsSupported ? finiteOrNull(initial.metrics.cls) : null,
    },
    availability: {
      decode: targetDecodeDurations.length
        ? "target-decode-marks"
        : decodeDurations.length
          ? "post-load-probe-only"
          : "unavailable",
      mainThread: targetMainThreadDurations.length
        ? "target-task-slices"
        : longTaskSupported
          ? "longtask-probe-only"
          : "unavailable",
      scene: scene.availability,
      lcp: lcpSupported ? "largest-contentful-paint-observer" : "unavailable",
      inp: inpSupported ? "event-timing-observer" : "unavailable",
      cls: clsSupported ? "layout-shift-session-window" : "unavailable",
    },
    violations: [...violations].sort(),
  };
  return run;
}

export function attachReceipt(run, receivedAt = new Date().toISOString()) {
  const sha256 = createHash("sha256").update(`${JSON.stringify(run)}\n`).digest("hex");
  return {
    ...run,
    server: {
      receivedAt,
      collectorVersion: COLLECTOR_VERSION,
      sha256,
    },
  };
}

export function injectProbe(html, nonce = "", requested = {}) {
  if (html.includes("data-pliego-route-lab")) return html;
  const nonceAttribute = nonce ? ` nonce="${escapeAttribute(nonce)}"` : "";
  const requestedConfiguration =
    typeof requested.tier === "string"
      ? `<script data-pliego-route-config${nonceAttribute}>` +
        `window.__PLIEGO_REQUESTED_TIER__=${inlineJson(requested.tier)};` +
        `window.__PLIEGO_REQUESTED_MOTION__=${inlineJson(requested.motionMode ?? "default")};` +
        `</script>`
      : "";
  const injection =
    requestedConfiguration +
    `<script defer src="/_pliego/probe-contract.js" data-pliego-route-contract${nonceAttribute}></script>` +
    `<script defer src="/_pliego/probe.js" data-pliego-route-lab${nonceAttribute}></script>`;
  const head = /<head(?:\s[^>]*)?>/i;
  if (head.test(html)) return html.replace(head, (match) => `${match}${injection}`);
  return `${injection}${html}`;
}

function inlineJson(value) {
  return JSON.stringify(String(value))
    .replaceAll("<", "\\u003c")
    .replaceAll("\u2028", "\\u2028")
    .replaceAll("\u2029", "\\u2029");
}

export function allowProbeNonce(csp, nonce) {
  if (!csp || !nonce) return csp;
  if (Array.isArray(csp)) {
    return csp.map((policy) => allowProbeNonce(policy, nonce));
  }
  const token = `'nonce-${nonce}'`;
  const policies = String(csp)
    .split(/,(?=\s*[a-z][a-z0-9-]*\s)/i)
    .map((policy) => policy.trim())
    .filter(Boolean);
  return policies
    .map((policy) => {
      const directives = policy
        .split(";")
        .map((directive) => directive.trim())
        .filter(Boolean);
      let foundScriptDirective = false;
      for (let index = 0; index < directives.length; index += 1) {
        const [name] = directives[index].split(/\s+/, 1);
        if (["script-src", "script-src-elem"].includes(name.toLowerCase())) {
          foundScriptDirective = true;
          if (!directives[index].includes(token)) directives[index] += ` ${token}`;
        }
      }
      if (!foundScriptDirective) directives.push(`script-src ${token}`);
      return `${directives.join("; ")};`;
    })
    .join(", ");
}

export function rewriteLocation(location, upstreamBase) {
  if (!location) return location;
  try {
    const base = new URL(upstreamBase);
    const resolved = new URL(location, base);
    if (resolved.origin !== base.origin) return location;
    return `${resolved.pathname}${resolved.search}${resolved.hash}`;
  } catch {
    return location;
  }
}

export function safeFilePart(value) {
  return String(value)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 72);
}

export function percentile(values, quantile) {
  const sorted = finiteNumbers(values).sort((left, right) => left - right);
  if (!sorted.length) return 0;
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * quantile) - 1);
  return round(sorted[index]);
}

function aggregateScene(segments, sceneHook, hasWebglCanvas) {
  const vram = segments
    .map((segment) => segment.metrics.estimatedVramBytes)
    .filter(Number.isFinite);
  if (sceneHook) {
    const drawCallSamples = segments.flatMap((segment) =>
      finiteNumbers(segment.metrics.drawCallSamples),
    );
    const triangleSamples = segments.flatMap((segment) =>
      finiteNumbers(segment.metrics.triangleSamples),
    );
    return {
      availability: "pliego-scene-hook",
      estimatedVramBytes: vram.length ? Math.max(...vram) : null,
      drawCalls: drawCallSamples.length ? Math.round(percentile(drawCallSamples, 0.95)) : null,
      triangles: triangleSamples.length ? Math.round(percentile(triangleSamples, 0.95)) : null,
    };
  }
  if (!hasWebglCanvas) {
    return {
      availability: "static-zero",
      estimatedVramBytes: vram.length ? Math.max(...vram) : 0,
      drawCalls: 0,
      triangles: 0,
    };
  }
  return {
    availability: "unavailable",
    estimatedVramBytes: vram.length ? Math.max(...vram) : null,
    drawCalls: null,
    triangles: null,
  };
}

function validateInitialSnapshot(snapshot, pagePath) {
  const errors = [];
  if (!snapshot || typeof snapshot !== "object" || Array.isArray(snapshot)) {
    return ["initialSnapshot is required"];
  }
  const expectedKeys = new Set([
    "capturedAt",
    "overflowed",
    "resources",
    "transferBytes",
    "encodedBodyBytes",
    "decodedBodyBytes",
    "resourceCount",
    "cachedResponseCount",
  ]);
  if (
    Object.keys(snapshot).length !== expectedKeys.size ||
    Object.keys(snapshot).some((key) => !expectedKeys.has(key))
  ) {
    errors.push("initialSnapshot fields are invalid");
  }
  if (!Number.isFinite(Date.parse(snapshot.capturedAt))) {
    errors.push("initialSnapshot.capturedAt is invalid");
  }
  if (typeof snapshot.overflowed !== "boolean") {
    errors.push("initialSnapshot.overflowed is invalid");
  }
  errors.push(...validateResourceLedger(snapshot.resources, pagePath));
  if (!Array.isArray(snapshot.resources)) return errors;
  if (snapshot.overflowed && snapshot.resources.length !== RESOURCE_LEDGER_LIMIT) {
    errors.push(`an overflowed initialSnapshot must retain ${RESOURCE_LEDGER_LIMIT} entries`);
  }
  const summary = summarizeResourceLedger(snapshot.resources);
  for (const field of [
    "transferBytes",
    "encodedBodyBytes",
    "decodedBodyBytes",
    "resourceCount",
    "cachedResponseCount",
  ]) {
    if (!Number.isSafeInteger(snapshot[field]) || snapshot[field] < 0) {
      errors.push(`initialSnapshot.${field} is invalid`);
    } else if (snapshot[field] !== summary[field]) {
      errors.push(
        `initialSnapshot.${field} does not reconcile with its resource ledger`,
      );
    }
  }
  return errors;
}

function validateResourceLedger(entries, expectedNavigationPath) {
  const errors = [];
  if (!Array.isArray(entries) || entries.length === 0 || entries.length > RESOURCE_LEDGER_LIMIT) {
    return [`resource ledger must contain between 1 and ${RESOURCE_LEDGER_LIMIT} entries`];
  }
  const expectedKeys = new Set([
    "entryType",
    "scope",
    "path",
    "initiator",
    "transferBytes",
    "encodedBodyBytes",
    "decodedBodyBytes",
    "cacheState",
    "durationMs",
  ]);
  entries.forEach((entry, index) => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
      errors.push(`resource ledger entry ${index} must be an object`);
      return;
    }
    if (
      Object.keys(entry).length !== expectedKeys.size ||
      Object.keys(entry).some((key) => !expectedKeys.has(key))
    ) {
      errors.push(`resource ledger entry ${index} fields are invalid`);
    }
    if (!resourceEntryTypes.has(entry.entryType)) {
      errors.push(`resource ledger entry ${index} entryType is invalid`);
    }
    if (!resourceScopes.has(entry.scope)) {
      errors.push(`resource ledger entry ${index} scope is invalid`);
    }
    if (!validLedgerPath(entry.path)) {
      errors.push(`resource ledger entry ${index} path is not a safe pathname`);
    }
    if (
      typeof entry.initiator !== "string" ||
      !entry.initiator ||
      entry.initiator.length > 64
    ) {
      errors.push(`resource ledger entry ${index} initiator is invalid`);
    }
    for (const field of ["transferBytes", "encodedBodyBytes", "decodedBodyBytes"]) {
      if (!Number.isSafeInteger(entry[field]) || entry[field] < 0) {
        errors.push(`resource ledger entry ${index} ${field} is invalid`);
      }
    }
    if (!cacheStates.has(entry.cacheState)) {
      errors.push(`resource ledger entry ${index} cacheState is invalid`);
    } else if (
      resourceScopes.has(entry.scope) &&
      [entry.transferBytes, entry.encodedBodyBytes, entry.decodedBodyBytes].every(
        (value) => Number.isSafeInteger(value) && value >= 0,
      )
    ) {
      const expectedCacheState = cacheStateForTiming(entry);
      if (entry.cacheState !== expectedCacheState) {
        errors.push(
          `resource ledger entry ${index} cacheState is ${entry.cacheState}, expected ${expectedCacheState}`,
        );
      }
    }
    if (!within(entry.durationMs, 0, 60 * 60 * 1000)) {
      errors.push(`resource ledger entry ${index} durationMs is invalid`);
    }
  });
  const navigationEntries = entries.filter((entry) => entry?.entryType === "navigation");
  if (navigationEntries.length !== 1) {
    errors.push("resource ledger must contain exactly one navigation entry");
  } else {
    const [navigation] = navigationEntries;
    if (navigation.scope !== "target-origin") {
      errors.push("resource ledger navigation must use target-origin scope");
    }
    if (
      validLedgerPath(expectedNavigationPath) &&
      normalizePath(navigation.path) !== normalizePath(expectedNavigationPath)
    ) {
      errors.push("resource ledger navigation path does not match the measured route");
    }
  }
  return errors;
}

function validLedgerPath(value) {
  return (
    typeof value === "string" &&
    value.startsWith("/") &&
    !value.startsWith("//") &&
    value.length <= 512 &&
    !/[?#]/.test(value)
  );
}

function normalizeSteps(value = {}) {
  return Object.fromEntries(
    [
      "ready",
      "scroll",
      "navigation",
      "visualResponse",
      "disclosure",
      "sceneHold",
      "returnToInitial",
    ].map((key) => [key, ["complete", "not-applicable"].includes(value[key]) ? value[key] : "missed"]),
  );
}

function finiteNumbers(values = []) {
  return values.filter(Number.isFinite).map(Number);
}

function finiteOrZero(value) {
  return Number.isFinite(value) ? Number(value) : 0;
}

function finiteOrNull(value) {
  return Number.isFinite(value) ? round(Number(value)) : null;
}

function sum(values) {
  return Math.round(values.reduce((total, value) => total + finiteOrZero(value), 0));
}

function normalizePath(value) {
  try {
    return new URL(value, "http://pliego.local").pathname.replace(/\/$/, "") || "/";
  } catch {
    return value;
  }
}

function round(value) {
  return Math.round(value * 100) / 100;
}

function within(value, minimum, maximum) {
  return Number.isFinite(value) && value >= minimum && value <= maximum;
}

function validMetricArray(value) {
  return (
    Array.isArray(value) &&
    value.length <= 20_000 &&
    value.every((item) => within(item, 0, 60_000))
  );
}

function validateExpectedViewport(session, actual, orientation, violations) {
  const expected = session.expectedViewport;
  if (!expected) return;
  let expectedWidth = expected.width;
  let expectedHeight = expected.height;
  if (
    expected.mode === "native" &&
    expected.orientation !== orientation &&
    Number.isFinite(expectedWidth) &&
    Number.isFinite(expectedHeight)
  ) {
    [expectedWidth, expectedHeight] = [expectedHeight, expectedWidth];
  }
  const tolerance = expected.mode === "fixed" ? 2 : 6;
  if (
    Number.isFinite(expectedWidth) &&
    Math.abs(finiteOrZero(actual.width) - expectedWidth) > tolerance
  ) {
    violations.add(`viewport-width-mismatch:${actual.width}`);
  }
  if (
    Number.isFinite(expectedHeight) &&
    !expectedHeightMatches({
      actualHeight: finiteOrZero(actual.height),
      expectedHeight,
      expectedMode: expected.mode,
      fingerprintViewport: session.expectedFingerprintViewport,
      orientation,
      tolerance,
    })
  ) {
    violations.add(`viewport-height-mismatch:${actual.height}`);
  }
  const expectedDpr = Number.isFinite(expected.deviceScaleFactor)
    ? expected.deviceScaleFactor
    : session.expectedFingerprintViewport?.devicePixelRatio;
  if (
    Number.isFinite(expectedDpr) &&
    Math.abs(finiteOrZero(actual.devicePixelRatio) - expectedDpr) > 0.05
  ) {
    violations.add(`viewport-dpr-mismatch:${actual.devicePixelRatio}`);
  }
}

function expectedHeightMatches({
  actualHeight,
  expectedHeight,
  expectedMode,
  fingerprintViewport,
  orientation,
  tolerance,
}) {
  if (expectedMode !== "native") {
    return Math.abs(actualHeight - expectedHeight) <= tolerance;
  }

  const fingerprintOrientation = String(fingerprintViewport?.orientation ?? "")
    .split("-")[0];
  const fingerprintMatchesOrientation =
    !fingerprintOrientation || fingerprintOrientation === orientation;
  const nativeHeights = fingerprintMatchesOrientation
    ? [
        fingerprintViewport?.innerHeight,
        fingerprintViewport?.visualHeight,
        fingerprintViewport?.availableHeight,
      ].filter(Number.isFinite)
    : [];

  if (!nativeHeights.length) {
    return Math.abs(actualHeight - expectedHeight) <= tolerance;
  }

  return within(
    actualHeight,
    Math.min(...nativeHeights) - tolerance,
    Math.max(...nativeHeights) + tolerance,
  );
}

function escapeAttribute(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}
