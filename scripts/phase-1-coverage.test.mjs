import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  aggregateAcceptedRuns,
  enumerateRequiredCases,
  evaluateBudgets,
  measurementCaseKey,
} from "./phase-1-coverage.mjs";

const plan = JSON.parse(
  await readFile(new URL("../fixtures/phase-1/measurement-plan.json", import.meta.url), "utf8"),
);
const deterministicGateSource = await readFile(
  new URL("./check-phase-1.mjs", import.meta.url),
  "utf8",
);
const closureGateSource = await readFile(
  new URL("./check-phase-1-closure.mjs", import.meta.url),
  "utf8",
);
const evidenceGateSource = await readFile(
  new URL("./phase-1-evidence.mjs", import.meta.url),
  "utf8",
);

test("the committed physical matrix expands to 180 aggregate reports", () => {
  const cases = enumerateRequiredCases(plan);
  assert.equal(cases.length, 180);
  assert.equal(new Set(cases.map(measurementCaseKey)).size, 180);
});

test("the iPad expands both declared orientations", () => {
  const orientations = new Set(
    enumerateRequiredCases(plan)
      .filter((item) => item.deviceId === "ipad-reference")
      .map((item) => item.orientation),
  );
  assert.deepEqual([...orientations].sort(), ["landscape", "portrait"]);
});

test("budget evaluation applies tier-specific and global limits", () => {
  const failures = evaluateBudgets(
    {
      tier: "lite",
      observations: {
        transferBytes: 1,
        estimatedVramBytes: plan.budgets.liteEstimatedVramBytes + 1,
        frameP95Ms: plan.budgets.liteFrameP95Ms + 1,
        lcpP75Ms: plan.budgets.lcpP75Ms + 1,
        inpP75Ms: plan.budgets.inpP75Ms + 1,
        clsP75: plan.budgets.clsP75 + 0.01,
      },
    },
    plan.budgets,
  );
  assert.equal(failures.length, 5);
});

test("accepted raw runs deterministically reproduce aggregate observations", () => {
  const runs = [10, 20, 30, 40, 50].map((value) => ({
    observations: {
      transferBytes: value,
      decodeP95Ms: value / 10,
      mainThreadP95Ms: value / 5,
      estimatedVramBytes: value * 100,
      drawCalls: value,
      triangles: value * 10,
      frameP95Ms: value / 2,
      lcpMs: value * 10,
      inpMs: value,
      cls: value / 1000,
    },
  }));
  assert.deepEqual(aggregateAcceptedRuns(runs), {
    transferBytes: 30,
    decodeP95Ms: 5,
    mainThreadP95Ms: 10,
    estimatedVramBytes: 5000,
    drawCalls: 50,
    triangles: 500,
    frameP95Ms: 25,
    lcpP75Ms: 400,
    inpP75Ms: 40,
    clsP75: 0.04,
  });
});

test("both Phase 1 gates share the accepted-evidence ledger validator", () => {
  assert.match(deterministicGateSource, /import \{ auditAcceptedEvidence \}/);
  assert.match(closureGateSource, /import \{ auditAcceptedEvidence \}/);
  assert.match(evidenceGateSource, /validateInitialRouteLedger\(run\)/);
});
