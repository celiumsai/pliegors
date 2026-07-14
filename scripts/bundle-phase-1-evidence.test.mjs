import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import {
  bundlePhase1Evidence,
  evidenceFields,
  normalizeTraceMetrics,
  parseCliArguments,
} from "./bundle-phase-1-evidence.mjs";
import { COLLECTOR_VERSION } from "../tools/route-lab/core.mjs";

const traceMetricsSchema = JSON.parse(
  await readFile(new URL("../schemas/pliego.trace-metrics.schema.json", import.meta.url), "utf8"),
);
const reportSchema = JSON.parse(
  await readFile(
    new URL("../schemas/pliego.measurement-report.schema.json", import.meta.url),
    "utf8",
  ),
);
const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);
const validateTraceMetrics = ajv.compile(traceMetricsSchema);

test("a complete evidence bundle binds artifacts, metrics, and five staged runs", async (t) => {
  const fixture = await createFixture("complete");
  t.after(() => rm(fixture.root, { recursive: true, force: true }));

  const { runsPath: _defaultRunsPath, ...defaultOptions } = fixture.options;
  const bundle = await bundlePhase1Evidence(defaultOptions);
  assert.equal(bundle.state, "complete");
  assert.deepEqual(bundle.runs, fixture.runRecords);
  assert.deepEqual(Object.keys(bundle.evidence), evidenceFields);
  assert.equal(bundle.evidence.collectorVersion, COLLECTOR_VERSION);
  assert.equal(
    bundle.evidence.rawRunsPath,
    `measurements/runs/accepted/${fixture.options.caseId}`,
  );
  assert.equal(
    bundle.evidence.tracePath,
    `measurements/evidence/${fixture.options.caseId}/trace.json.gz`,
  );
  assert.equal(
    bundle.evidence.traceMetricsPath,
    `measurements/evidence/${fixture.options.caseId}/trace-metrics.json`,
  );

  const bundleRoot = path.join(
    fixture.root,
    "measurements",
    "evidence",
    fixture.options.caseId,
  );
  const storedMetricsBytes = await readFile(path.join(bundleRoot, "trace-metrics.json"));
  const storedMetrics = JSON.parse(storedMetricsBytes);
  assert.equal(validateTraceMetrics(storedMetrics), true, ajv.errorsText(validateTraceMetrics.errors));
  assert.deepEqual(
    storedMetrics.runs.map((run) => run.sessionId),
    fixture.sessionIds,
  );
  assert.equal(bundle.evidence.traceMetricsSha256, sha256(storedMetricsBytes));
  assert.deepEqual(
    JSON.parse(await readFile(path.join(bundleRoot, "evidence.json"), "utf8")),
    bundle.evidence,
  );
  assert.deepEqual(
    JSON.parse(await readFile(path.join(bundleRoot, "bundle.json"), "utf8")),
    bundle,
  );

  const replay = await bundlePhase1Evidence(defaultOptions);
  assert.deepEqual(replay, bundle);
});

test("an existing complete bundle is immutable", async (t) => {
  const fixture = await createFixture("immutable");
  t.after(() => rm(fixture.root, { recursive: true, force: true }));
  const first = await bundlePhase1Evidence(fixture.options);
  await writeFile(fixture.options.screenshotPath, Buffer.from("changed screenshot"));
  await assert.rejects(
    bundlePhase1Evidence(fixture.options),
    /already exists with different immutable metadata/,
  );
  const stored = JSON.parse(
    await readFile(
      path.join(
        fixture.root,
        "measurements",
        "evidence",
        fixture.options.caseId,
        "bundle.json",
      ),
      "utf8",
    ),
  );
  assert.deepEqual(stored, first);
});

test("trace metrics must bind the trace and the exact staged session set", async (t) => {
  const fixture = await createFixture("binding");
  t.after(() => rm(fixture.root, { recursive: true, force: true }));
  const metrics = JSON.parse(await readFile(fixture.options.metricsPath, "utf8"));
  metrics.sourceTraceSha256 = "0".repeat(64);
  await writeFile(fixture.options.metricsPath, JSON.stringify(metrics));
  await assert.rejects(
    bundlePhase1Evidence(fixture.options),
    /sourceTraceSha256 does not match/,
  );

  metrics.sourceTraceSha256 = fixture.traceSha256;
  metrics.runs[0].sessionId = "f".repeat(24);
  await writeFile(fixture.options.metricsPath, JSON.stringify(metrics));
  await assert.rejects(
    bundlePhase1Evidence(fixture.options),
    /sessionIds do not match the staged raw run set/,
  );
});

test("trace metrics reject aggregate or diagnostic substitutions", () => {
  const traceSha256 = "a".repeat(64);
  const sessionIds = Array.from({ length: 5 }, (_, index) => sessionId(index + 1));
  assert.throws(
    () =>
      normalizeTraceMetrics(
        {
          metricsVersion: "1.0.0",
          extractorVersion: "pliego-trace-extractor/0.1.0",
          sourceTraceSha256: traceSha256,
          decodeP95Ms: 1,
          mainThreadP95Ms: 2,
        },
        traceSha256,
        sessionIds,
      ),
    /must contain exactly/,
  );
  const value = metricsDocument(traceSha256, sessionIds);
  value.runs[0].decodeDurationsMs = [];
  assert.throws(
    () => normalizeTraceMetrics(value, traceSha256, sessionIds),
    /must contain at least one sample/,
  );
  value.runs[0].decodeDurationsMs = Array.from({ length: 20_001 }, () => 1);
  assert.throws(
    () => normalizeTraceMetrics(value, traceSha256, sessionIds),
    /must contain at most 20000 samples/,
  );
});

test("the CLI requires the three artifacts and accepts an optional staging directory", () => {
  assert.deepEqual(
    parseCliArguments([
      "--case-id",
      "android-site-root-universal-cold-default",
      "--trace",
      "trace.json.gz",
      "--screenshot",
      "screen.png",
      "--metrics",
      "metrics.json",
      "--runs",
      "staging",
    ]),
    {
      help: false,
      caseId: "android-site-root-universal-cold-default",
      tracePath: "trace.json.gz",
      screenshotPath: "screen.png",
      metricsPath: "metrics.json",
      runsPath: "staging",
      collectorVersion: COLLECTOR_VERSION,
    },
  );
  assert.throws(
    () => parseCliArguments(["--case-id", "case", "--trace", "trace.json"]),
    /--screenshot is required/,
  );
});

test("report evidence requires the trace metrics artifact and hash", () => {
  const required = reportSchema.$defs.evidence.required;
  assert.deepEqual(required, evidenceFields);
  assert.ok(reportSchema.$defs.evidence.properties.traceMetricsPath);
  assert.ok(reportSchema.$defs.evidence.properties.traceMetricsSha256);
});

async function createFixture(suffix) {
  const root = await mkdtemp(path.join(os.tmpdir(), `pliego-evidence-${suffix}-`));
  const incoming = path.join(root, "incoming");
  const caseId = `android-site-root-${suffix}`;
  const runsPath = path.join(root, "measurements", "runs", "staging", caseId);
  await Promise.all([
    mkdir(incoming, { recursive: true }),
    mkdir(runsPath, { recursive: true }),
  ]);
  const tracePath = path.join(incoming, "capture.json.gz");
  const screenshotPath = path.join(incoming, "capture.png");
  const metricsPath = path.join(incoming, "metrics.json");
  const traceBytes = Buffer.from("deterministic trace bytes");
  await Promise.all([
    writeFile(tracePath, traceBytes),
    writeFile(screenshotPath, Buffer.from("deterministic screenshot bytes")),
  ]);
  const traceSha256 = sha256(traceBytes);
  const sessionIds = Array.from({ length: 5 }, (_, index) => sessionId(index + 1));
  const runRecords = [];
  for (let index = 0; index < sessionIds.length; index += 1) {
    const fileName = `run-${index + 1}.json`;
    const bytes = Buffer.from(
      `${JSON.stringify(
        { sessionId: sessionIds[index], server: { collectorVersion: COLLECTOR_VERSION } },
        null,
        2,
      )}\n`,
    );
    await writeFile(path.join(runsPath, fileName), bytes);
    runRecords.push({ fileName, sessionId: sessionIds[index], sha256: sha256(bytes) });
  }
  const metrics = metricsDocument(traceSha256, [...sessionIds].reverse());
  await writeFile(metricsPath, JSON.stringify(metrics));
  return {
    root,
    traceSha256,
    sessionIds,
    runRecords,
    options: {
      repoRoot: root,
      caseId,
      tracePath,
      screenshotPath,
      metricsPath,
      runsPath,
    },
  };
}

function metricsDocument(sourceTraceSha256, sessionIds) {
  return {
    runs: sessionIds.map((id, index) => ({
      mainThreadTaskDurationsMs: [index + 2, index + 2.5],
      sessionId: id,
      decodeDurationsMs: [index + 1, index + 1.5],
    })),
    sourceTraceSha256,
    extractorVersion: "pliego-trace-extractor/0.1.0",
    metricsVersion: "1.0.0",
  };
}

function sessionId(index) {
  return index.toString(16).padStart(24, "0");
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}
