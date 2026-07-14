import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { cacheStateForTiming as serverCacheStateForTiming } from "./core.mjs";

const source = readFileSync(new URL("./probe-contract.js", import.meta.url), "utf8");
const sandbox = { URL };
vm.runInNewContext(source, sandbox);
const logic = sandbox.__PLIEGO_ROUTE_PROBE_LOGIC__;

test("first-viewport capture waits for exactly two animation frames", () => {
  const queued = [];
  let captured = false;
  logic.afterTwoAnimationFrames(
    (callback) => queued.push(callback),
    () => {
      captured = true;
    },
  );
  assert.equal(captured, false);
  assert.equal(queued.length, 1);
  queued.shift()();
  assert.equal(captured, false);
  assert.equal(queued.length, 1);
  queued.shift()();
  assert.equal(captured, true);
});

test("normal pagehide remains foreground while a confirmed background does not", () => {
  assert.equal(
    logic.foregroundAtCapture({
      leftForeground: false,
      visibilityState: "hidden",
      pageHiding: true,
    }),
    true,
  );
  assert.equal(
    logic.foregroundAtCapture({
      leftForeground: true,
      visibilityState: "hidden",
      pageHiding: false,
    }),
    false,
  );
});

test("authored disclosure and button clicks map to distinct interaction steps", () => {
  assert.equal(
    logic.classifyAuthoredClick({
      insideOverlay: false,
      disclosureControl: true,
      buttonControl: true,
    }),
    "disclosure",
  );
  assert.equal(
    logic.classifyAuthoredClick({
      insideOverlay: false,
      disclosureControl: false,
      buttonControl: true,
    }),
    "visualResponse",
  );
  assert.equal(
    logic.classifyAuthoredClick({
      insideOverlay: true,
      disclosureControl: true,
      buttonControl: true,
    }),
    null,
  );
});

test("scene hold is derived only from observed scene presence", () => {
  assert.equal(
    logic.resolveSceneHold({ hasScene: true }),
    "complete",
  );
  assert.equal(
    logic.resolveSceneHold({ hasScene: false }),
    "not-applicable",
  );
});

test("finish gate requires both the minimum duration and every required step", () => {
  const completeSteps = {
    ready: "complete",
    scroll: "complete",
    navigation: "complete",
    visualResponse: "complete",
    disclosure: "not-applicable",
    sceneHold: "not-applicable",
    returnToInitial: "complete",
  };
  const tooEarly = logic.evaluateFinishGate({
    elapsedMs: 9_250,
    minimumDurationMs: 10_000,
    steps: completeSteps,
  });
  assert.equal(tooEarly.ready, false);
  assert.equal(tooEarly.minimumMet, false);
  assert.equal(tooEarly.remainingMs, 750);
  assert.equal(tooEarly.missingSteps.length, 0);

  const missingReturn = logic.evaluateFinishGate({
    elapsedMs: 10_000,
    minimumDurationMs: 10_000,
    steps: { ...completeSteps, returnToInitial: "missed" },
  });
  assert.equal(missingReturn.ready, false);
  assert.equal(missingReturn.minimumMet, true);
  assert.deepEqual(Array.from(missingReturn.missingSteps), ["returnToInitial"]);

  const ready = logic.evaluateFinishGate({
    elapsedMs: 10_001,
    minimumDurationMs: 10_000,
    steps: completeSteps,
  });
  assert.equal(ready.ready, true);
  assert.equal(ready.remainingMs, 0);
});

test("finish gate treats absent and unknown required states as incomplete", () => {
  const result = logic.evaluateFinishGate({
    elapsedMs: 20_000,
    minimumDurationMs: 10_000,
    steps: { ready: "complete", scroll: "invalid" },
  });
  assert.equal(result.ready, false);
  assert.deepEqual(Array.from(result.missingSteps), [
    "scroll",
    "navigation",
    "visualResponse",
    "disclosure",
    "sceneHold",
    "returnToInitial",
  ]);
});

test("an operator rejection can seal while an incomplete Finish action cannot", () => {
  assert.equal(
    logic.canSealRun({ finishReady: false, operatorRejected: false }),
    false,
  );
  assert.equal(
    logic.canSealRun({ finishReady: true, operatorRejected: false }),
    true,
  );
  assert.equal(
    logic.canSealRun({ finishReady: false, operatorRejected: true }),
    true,
  );
});

test("development server markers are rejected without flagging production assets", () => {
  assert.equal(
    logic.hasDevelopmentServerArtifact({
      scriptSources: ["http://127.0.0.1:5274/@vite/client"],
    }),
    true,
  );
  assert.equal(
    logic.hasDevelopmentServerArtifact({
      scriptSources: ["/assets/index-Ck8vh59h.js"],
    }),
    false,
  );
  assert.equal(logic.hasDevelopmentServerArtifact({ hasAstroToolbar: true }), true);
});

test("resource timing cache states distinguish network, local, validated, and opaque", () => {
  const cases = [
    {
      timing: {
        scope: "target-origin",
        transferBytes: 900,
        encodedBodyBytes: 600,
        decodedBodyBytes: 1200,
      },
      expected: "network",
    },
    {
      timing: {
        scope: "target-origin",
        transferBytes: 0,
        encodedBodyBytes: 0,
        decodedBodyBytes: 1200,
      },
      expected: "local-cache",
    },
    {
      timing: {
        scope: "target-origin",
        transferBytes: 300,
        encodedBodyBytes: 0,
        decodedBodyBytes: 1200,
      },
      expected: "validated-cache",
    },
    {
      timing: {
        scope: "external",
        transferBytes: 0,
        encodedBodyBytes: 0,
        decodedBodyBytes: 0,
      },
      expected: "opaque",
    },
    {
      timing: {
        scope: "target-origin",
        transferBytes: 0,
        encodedBodyBytes: 0,
        decodedBodyBytes: 0,
      },
      expected: "unknown",
    },
  ];

  for (const { timing, expected } of cases) {
    assert.equal(logic.cacheStateForTiming(timing), expected);
    assert.equal(serverCacheStateForTiming(timing), expected);
  }
});

test("resource paths discard credentials, origin, query, and fragment", () => {
  assert.equal(
    logic.resourcePath(
      "https://user:secret@example.com/assets/app.js?token=private#part",
      "https://pliego.local/",
    ),
    "/assets/app.js",
  );
});
