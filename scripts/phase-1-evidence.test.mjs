import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { auditAcceptedEvidence } from "./phase-1-evidence.mjs";
import { aggregateAcceptedRuns } from "./phase-1-coverage.mjs";

test("a loose accepted-run JSON is rejected and never counted", async (t) => {
  const fixture = await createFixture(t, { reports: false });
  await writeFile(path.join(fixture.acceptedRunRoot, "loose.json"), "{}\n");

  const audit = await runAudit(fixture, []);

  assert.equal(audit.acceptedReportCount, 0);
  assert.equal(audit.acceptedRunCount, 0);
  assert.match(audit.errors.join("\n"), /loose\.json is a loose JSON/);
});

test("missing report artifacts prevent a five-run case from counting", async (t) => {
  const fixture = await createFixture(t, { artifacts: false });

  const audit = await runAudit(fixture, [fixture.reportRecord]);

  assert.equal(audit.acceptedReportCount, 0);
  assert.equal(audit.acceptedRunCount, 0);
  assert.match(audit.errors.join("\n"), /trace is missing/);
  assert.match(audit.errors.join("\n"), /screenshot is missing/);
  assert.match(audit.errors.join("\n"), /trace metrics is missing/);
});

test("artifact hashes are verified before a report counts", async (t) => {
  const fixture = await createFixture(t);
  fixture.reportRecord.value.evidence.traceSha256 = "0".repeat(64);

  const audit = await runAudit(fixture, [fixture.reportRecord]);

  assert.equal(audit.acceptedReportCount, 0);
  assert.equal(audit.acceptedRunCount, 0);
  assert.match(audit.errors.join("\n"), /trace SHA-256 does not match/);
});

test("one referenced case directory with five sealed runs and artifacts counts", async (t) => {
  const fixture = await createFixture(t);

  const audit = await runAudit(fixture, [fixture.reportRecord]);

  assert.deepEqual(audit.errors, []);
  assert.equal(audit.acceptedReportCount, 1);
  assert.equal(audit.acceptedRunCount, 5);
  assert.equal(audit.reviews[0].accepted, true);
  assert.equal(fixture.reportRecord.value.observations.decodeP95Ms, 5);
  assert.equal(fixture.reportRecord.value.observations.mainThreadP95Ms, 10);
});

test("trace metrics must bind exactly the five accepted session IDs", async (t) => {
  const fixture = await createFixture(t);
  fixture.reportRecord.value.evidence.traceMetricsPath =
    "measurements/evidence/foreign-trace-metrics.json";
  const metrics = structuredClone(fixture.traceMetrics);
  metrics.runs[0].sessionId = "f".repeat(24);
  const metricsBytes = Buffer.from(`${JSON.stringify(metrics)}\n`);
  await writeFile(path.join(fixture.repoRoot, fixture.reportRecord.value.evidence.traceMetricsPath), metricsBytes);
  fixture.reportRecord.value.evidence.traceMetricsSha256 = sha256(metricsBytes);

  const audit = await runAudit(fixture, [fixture.reportRecord]);

  assert.equal(audit.acceptedReportCount, 0);
  assert.equal(audit.acceptedRunCount, 0);
  assert.match(audit.errors.join("\n"), /is missing raw-run sessionId/);
  assert.match(audit.errors.join("\n"), /contains foreign sessionId/);
});

test("all five runs must use the report evidence collector version", async (t) => {
  const fixture = await createFixture(t);
  const changedRun = fixture.runs[2];
  changedRun.server.collectorVersion = "test-collector/2.0.0";
  await writeFile(
    path.join(fixture.caseDirectory, "run-3.json"),
    `${JSON.stringify(changedRun, null, 2)}\n`,
  );

  const audit = await runAudit(fixture, [fixture.reportRecord]);

  assert.equal(audit.acceptedReportCount, 0);
  assert.equal(audit.acceptedRunCount, 0);
  assert.match(audit.errors.join("\n"), /run-3\.json collector version differs/);
});

async function createFixture(t, { reports = true, artifacts = true } = {}) {
  const repoRoot = await mkdtemp(path.join(tmpdir(), "pliego-evidence-"));
  t.after(() => rm(repoRoot, { recursive: true, force: true }));
  const acceptedRunRoot = path.join(repoRoot, "measurements", "runs", "accepted");
  const acceptedFingerprintRoot = path.join(repoRoot, "measurements", "accepted");
  await mkdir(acceptedRunRoot, { recursive: true });
  await mkdir(acceptedFingerprintRoot, { recursive: true });

  const fingerprintName = "test-device.json";
  const fingerprintBytes = Buffer.from('{"deviceId":"test-device"}\n');
  await writeFile(path.join(acceptedFingerprintRoot, fingerprintName), fingerprintBytes);
  const hardwareFingerprint = `${fingerprintName}#sha256=${sha256(fingerprintBytes)}`;
  const sourceRevision = `html-sha256:${"a".repeat(64)};manifest-sha256:${"b".repeat(64)}`;
  const caseDirectory = path.join(acceptedRunRoot, "test-case");
  await mkdir(caseDirectory, { recursive: true });

  const runs = [];
  for (let index = 1; index <= 5; index += 1) {
    const transferBytes = 100 + index;
    const run = {
      sessionId: index.toString(16).padStart(24, "0"),
      deviceId: "test-device",
      targetId: "test-target",
      route: "/",
      tier: "universal",
      cacheMode: "cold",
      motionMode: "default",
      viewport: { orientation: "portrait" },
      hardwareFingerprint,
      browserFingerprint: "Test Browser 1",
      sourceRevision,
      initialRouteResources: [
        {
          entryType: "navigation",
          scope: "target-origin",
          path: "/",
          initiator: "navigation",
          transferBytes,
          encodedBodyBytes: transferBytes - 1,
          decodedBodyBytes: transferBytes + 10,
          cacheState: "network",
          durationMs: 10,
        },
      ],
      observations: {
        transferBytes,
        encodedBodyBytes: transferBytes - 1,
        decodedBodyBytes: transferBytes + 10,
        resourceCount: 0,
        cachedResponseCount: 0,
        decodeP95Ms: null,
        mainThreadP95Ms: null,
        estimatedVramBytes: index * 1000,
        drawCalls: index,
        triangles: index * 10,
        frameP95Ms: 15 + index,
        lcpMs: 200 + index,
        inpMs: 20 + index,
        cls: index / 1000,
      },
      violations: [],
    };
    run.server = {
      collectorVersion: "test-collector/1.0.0",
      sha256: receiptFor(run),
    };
    runs.push(run);
    await writeFile(
      path.join(caseDirectory, `run-${index}.json`),
      `${JSON.stringify(run, null, 2)}\n`,
    );
  }

  const tracePath = "measurements/evidence/trace.json";
  const traceMetricsPath = "measurements/evidence/trace-metrics.json";
  const screenshotPath = "measurements/evidence/screenshot.png";
  const traceBytes = Buffer.from('{"traceEvents":[]}\n');
  const traceMetrics = {
    metricsVersion: "1.0.0",
    extractorVersion: "test-extractor/1.0.0",
    sourceTraceSha256: sha256(traceBytes),
    runs: runs.map((run, index) => ({
      sessionId: run.sessionId,
      decodeDurationsMs: [0.25, index + 1],
      mainThreadTaskDurationsMs: [0.5, (index + 1) * 2],
    })),
  };
  const traceMetricsBytes = Buffer.from(`${JSON.stringify(traceMetrics)}\n`);
  const runsWithTraceMetrics = runs.map((run, index) => ({
    ...run,
    observations: {
      ...run.observations,
      decodeP95Ms: index + 1,
      mainThreadP95Ms: (index + 1) * 2,
    },
  }));
  const screenshotBytes = Buffer.from("test screenshot");
  if (artifacts) {
    await mkdir(path.join(repoRoot, "measurements", "evidence"), { recursive: true });
    await writeFile(path.join(repoRoot, tracePath), traceBytes);
    await writeFile(path.join(repoRoot, traceMetricsPath), traceMetricsBytes);
    await writeFile(path.join(repoRoot, screenshotPath), screenshotBytes);
  }

  const reportRecord = reports
    ? {
        fileName: "test-report.json",
        value: {
          deviceId: "test-device",
          targetId: "test-target",
          route: "/",
          tier: "universal",
          orientation: "portrait",
          cacheMode: "cold",
          motionMode: "default",
          hardwareFingerprint,
          browserFingerprint: "Test Browser 1",
          sourceRevision,
          runCount: 5,
          observations: aggregateAcceptedRuns(runsWithTraceMetrics),
          evidence: {
            collectorVersion: "test-collector/1.0.0",
            tracePath,
            traceSha256: sha256(traceBytes),
            traceMetricsPath,
            traceMetricsSha256: sha256(traceMetricsBytes),
            screenshotPath,
            screenshotSha256: sha256(screenshotBytes),
            rawRunsPath: "measurements/runs/accepted/test-case",
          },
        },
      }
    : null;

  return {
    repoRoot,
    acceptedRunRoot,
    acceptedFingerprintRoot,
    caseDirectory,
    reportRecord,
    runs,
    traceMetrics,
  };
}

function runAudit(fixture, reports) {
  const validateRun = () => true;
  return auditAcceptedEvidence({
    repoRoot: fixture.repoRoot,
    acceptedRunRoot: fixture.acceptedRunRoot,
    acceptedFingerprintRoot: fixture.acceptedFingerprintRoot,
    reports,
    measuredRuns: 5,
    validateRun,
  });
}

function receiptFor(run) {
  const withoutReceipt = { ...run };
  delete withoutReceipt.server;
  return sha256(`${JSON.stringify(withoutReceipt)}\n`);
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}
