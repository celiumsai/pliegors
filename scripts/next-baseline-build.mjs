// SPDX-License-Identifier: AGPL-3.0-only

import { cp, mkdir, mkdtemp, readFile, rm, stat } from "node:fs/promises";
import path from "node:path";
import { spawn as spawnChild, spawnSync } from "node:child_process";
import { nearestRankSummary } from "./next-baseline-lib.mjs";
import { createProcessTreeRssSampler } from "./process-tree-rss.mjs";

const BUILD_RECORD_KEYS = [
  "recordVersion",
  "outcome",
  "changedSources",
  "globalInvalidation",
  "renderedArtifacts",
  "reusedArtifacts",
  "receiptBefore",
  "receiptAfter",
  "phases",
];
const PHASE_KEYS = [
  "discoveryMicros",
  "clientMicros",
  "siteMicros",
  "verificationMicros",
  "totalMicros",
];
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const MAX_PROCESS_OUTPUT_BYTES = 32 * 1024 * 1024;

export function parseBuildRecord(value) {
  assertPlainObject(value, "build record");
  assertExactKeys(value, BUILD_RECORD_KEYS, "build record");
  assert(value.recordVersion === "pliego-build/1", "build record version differs");
  assert(["executed", "no-op"].includes(value.outcome), "build record outcome is invalid");
  assert(Array.isArray(value.changedSources) && value.changedSources.every(nonEmptyString), "build changedSources are invalid");
  assertUnique(value.changedSources, "build changed source");
  assertSorted(value.changedSources, "build changedSources");
  assert(typeof value.globalInvalidation === "boolean", "build globalInvalidation is invalid");
  assertSafeCount(value.renderedArtifacts, "renderedArtifacts");
  assertSafeCount(value.reusedArtifacts, "reusedArtifacts");
  assert(value.receiptBefore === null || SHA256_PATTERN.test(value.receiptBefore), "build receiptBefore is invalid");
  assert(SHA256_PATTERN.test(value.receiptAfter), "build receiptAfter is invalid");
  assertPlainObject(value.phases, "build phases");
  assertExactKeys(value.phases, PHASE_KEYS, "build phases");
  for (const key of PHASE_KEYS) assertSafeCount(value.phases[key], `build ${key}`);
  const phaseTotal = value.phases.discoveryMicros
    + value.phases.clientMicros
    + value.phases.siteMicros
    + value.phases.verificationMicros;
  assert(Number.isSafeInteger(phaseTotal) && phaseTotal <= value.phases.totalMicros, "build phase durations exceed totalMicros");
  return structuredClone(value);
}

export function parseBuildLedger(value) {
  assertPlainObject(value, "build ledger");
  assert(value.reportVersion === "2.0.0", "build report version differs");
  assert(SHA256_PATTERN.test(value.receiptSha256), "build receipt SHA-256 is invalid");
  assertPlainObject(value.receipt, "artifact receipt");
  assert(value.receipt.receiptVersion === "2.0.0", "artifact receipt version differs");
  const outputs = value.receipt.outputs;
  assertPlainObject(outputs, "output set");
  assert(Array.isArray(outputs.files), "output files are invalid");
  assertSafeCount(outputs.fileCount, "output fileCount");
  assertSafeCount(outputs.totalBytes, "output totalBytes");
  assert(SHA256_PATTERN.test(outputs.sha256), "output-set SHA-256 is invalid");
  assert(outputs.files.length === outputs.fileCount, "output fileCount differs from files");

  let totalBytes = 0;
  let wasmBytes = 0;
  let assetBytes = 0;
  const paths = [];
  for (const file of outputs.files) {
    assertPlainObject(file, "output file");
    assertExactKeys(file, ["path", "kind", "producer", "bytes", "sha256"], "output file");
    assert(nonEmptyString(file.path) && !file.path.includes("\\") && !file.path.startsWith("/"), "output path is invalid");
    assert(nonEmptyString(file.kind) && nonEmptyString(file.producer), "output metadata is invalid");
    assertSafeCount(file.bytes, `output bytes for ${file.path}`);
    assert(SHA256_PATTERN.test(file.sha256), `output SHA-256 is invalid for ${file.path}`);
    paths.push(file.path);
    totalBytes = checkedAdd(totalBytes, file.bytes, "output total bytes overflow");
    if (file.path.endsWith(".wasm")) wasmBytes = checkedAdd(wasmBytes, file.bytes, "WASM bytes overflow");
    if (file.kind === "asset") assetBytes = checkedAdd(assetBytes, file.bytes, "asset bytes overflow");
  }
  assertUnique(paths, "output path");
  assertSorted(paths, "output paths");
  assert(totalBytes === outputs.totalBytes, "output totalBytes differs from file bytes");
  return {
    receiptSha256: value.receiptSha256,
    fileCount: outputs.fileCount,
    totalBytes,
    wasmBytes,
    assetBytes,
  };
}

export function projectFixtureBuildMetrics(samples, manifest) {
  assert(samples.length === manifest.policy.measuredRuns, "fixture sample count differs from manifest policy");
  for (const [index, sample] of samples.entries()) validateRawSample(sample, index + 1);
  const definitions = new Map(manifest.measurementCases.map((item) => [item.id, item]));
  const metrics = [
    measuredMetric("build-cold", samples, (sample) => sample.cold.durationMs, definitions),
    measuredMetric("build-warm-verified", samples, (sample) => sample.warm.durationMs, definitions),
    measuredMetric("phase-discovery", samples, (sample) => microsToMilliseconds(sample.cold.record.phases.discoveryMicros), definitions),
    measuredMetric("phase-rust-wasm", samples, (sample) => microsToMilliseconds(sample.cold.record.phases.clientMicros), definitions),
    measuredMetric("phase-site", samples, (sample) => microsToMilliseconds(sample.cold.record.phases.siteMicros), definitions),
    measuredMetric("phase-verification", samples, (sample) => microsToMilliseconds(sample.cold.record.phases.verificationMicros), definitions),
    measuredMetric("wasm-bytes", samples, (sample) => sample.output.wasmBytes, definitions),
    measuredMetric("asset-bytes", samples, (sample) => sample.output.assetBytes, definitions),
    measuredMetric("cache-reused-artifacts", samples, (sample) => sample.warm.record.reusedArtifacts, definitions),
  ];
  if (samples.every((sample) => Number.isSafeInteger(sample.cold.peakRssBytes) && sample.cold.peakRssBytes > 0)) {
    metrics.push(measuredMetric("peak-rss", samples, (sample) => sample.cold.peakRssBytes, definitions));
  }
  return metrics;
}

export function applyFixtureBuildMeasurements(report, executions) {
  const projected = structuredClone(report);
  assertUnique(executions.map((execution) => execution.fixtureId), "executed fixture ID");
  for (const execution of executions) {
    const fixture = projected.fixtures.find((candidate) => candidate.id === execution.fixtureId);
    assert(fixture, `baseline report has no fixture ${execution.fixtureId}`);
    const byId = new Map(execution.metrics.map((metric) => [metric.caseId, metric]));
    assert(byId.size === execution.metrics.length, `duplicate ${execution.fixtureId} metric`);
    fixture.results = fixture.results.map((result) => applyBuildResult(result, byId, execution));
  }
  const executedNames = executions.map((execution) => execution.fixtureId).join(", ");
  const unexecuted = projected.fixtures
    .map((fixture) => fixture.id)
    .filter((id) => !executions.some((execution) => execution.fixtureId === id));
  const executionVerb = executions.length === 1 ? "contributes" : "contribute";
  const remainderVerb = unexecuted.length === 1 ? "remains" : "remain";
  projected.status = "incomplete";
  projected.bottlenecks = [];
  projected.limitations = [
    `${executedNames} ${executionVerb} real build, phase, emitted-asset, and verified-reuse measurements.`,
    `${unexecuted.join(", ")} ${remainderVerb} without build measurements, so this report is not a complete Phase 0 baseline.`,
    "No cache attempt/hit/miss rate is inferred from rendered and reused artifact counts.",
    "No development, browser-visible, lifecycle-inspector, transform, remount, or reload metric is claimed without its collector.",
  ];
  return projected;
}

function applyBuildResult(result, byId, execution) {
  const measured = byId.get(result.caseId);
  if (measured) return measured;
  if (result.status === "not-applicable") return result;
  if (result.caseId === "peak-rss" && !execution.peakRssMeasured) {
    return {
      caseId: result.caseId,
      unit: result.unit,
      status: "unavailable",
      reasonCode: "PLATFORM_COLLECTOR_UNAVAILABLE",
      explanation: "Process-tree peak RSS is currently collected only through Linux procfs.",
    };
  }
  if (["cache-attempts", "cache-hits", "cache-misses", "cache-hit-rate"].includes(result.caseId)) {
    return {
      caseId: result.caseId,
      unit: result.unit,
      status: "unavailable",
      reasonCode: "COLLECTOR_NOT_IMPLEMENTED",
      explanation: "The current build record exposes verified reuse, not actual cache lookup attempts, hits, or misses.",
    };
  }
  return {
    caseId: result.caseId,
    unit: result.unit,
    status: "unavailable",
    reasonCode: "COLLECTOR_NOT_IMPLEMENTED",
    explanation: `${execution.fixtureId} is executable, but this measurement requires a development, browser, lifecycle, transform, or HMR collector that is not implemented yet.`,
  };
}

export async function collectFixtureBuildSamples({
  root,
  fixture,
  manifest,
  cli,
  keep = false,
}) {
  const descriptor = JSON.parse(await readFile(path.resolve(root, ...fixture.descriptor.split("/")), "utf8"));
  assert(descriptor.stage === "executable", `${fixture.id} fixture is not executable`);
  const fixtureRoot = path.resolve(root, ...fixture.root.split("/"));
  // target/<temp>/application has the same depth as fixtures/next/<id>,
  // preserving each fixture's reviewed path dependencies.
  const workspaceParent = path.join(root, "target");
  await mkdir(workspaceParent, { recursive: true });
  const runRoots = [];
  const validationTarget = path.join(workspaceParent, `.next-build-validation-${fixture.id}-${process.pid}-${Date.now()}`);
  const samples = [];
  const totalRuns = manifest.policy.warmupRuns + manifest.policy.measuredRuns;
  try {
    for (let runIndex = 1; runIndex <= totalRuns; runIndex += 1) {
      const sampleRoot = await mkdtemp(path.join(workspaceParent, `next-${fixture.id}-run-`));
      runRoots.push(sampleRoot);
      const project = path.join(sampleRoot, "application");
      const cargoTarget = path.join(sampleRoot, "cargo-target");
      await cp(fixtureRoot, project, {
        recursive: true,
        filter: (source) => {
          const relative = path.relative(fixtureRoot, source);
          return relative !== "target" && !relative.startsWith(`target${path.sep}`);
        },
      });
      const validationEnvironment = { CARGO_TARGET_DIR: validationTarget };
      run(cli, ["check"], project, validationEnvironment);
      run("cargo", ["test", "--locked", "--quiet", "--workspace"], project, validationEnvironment);
      assert(!(await pathExists(cargoTarget)), `${fixture.id} run ${runIndex}: cold Cargo target was not empty`);

      const environment = { CARGO_TARGET_DIR: cargoTarget };
      const cold = await measured(cli, ["build"], project, environment);
      const coldRecord = parseBuildRecord(JSON.parse(run(cli, ["cache", "status", "--format", "json"], project, environment)));
      validateCold(cold, coldRecord, fixture.id, runIndex);

      const warm = await measured(cli, ["build"], project, environment);
      const warmRecord = parseBuildRecord(JSON.parse(run(cli, ["cache", "status", "--format", "json"], project, environment)));
      validateWarm(warm, coldRecord, warmRecord, fixture.id, runIndex);
      run(cli, ["inspect"], project, environment);

      const ledger = parseBuildLedger(JSON.parse(await readFile(path.join(project, "target", "site", "pliego.build.json"), "utf8")));
      assert(ledger.receiptSha256 === warmRecord.receiptAfter, `${fixture.id} run ${runIndex}: ledger receipt differs from warm record`);
      const sample = {
        sample: runIndex - manifest.policy.warmupRuns,
        cold: { durationMs: cold.durationMs, peakRssBytes: cold.peakRssBytes, record: coldRecord },
        warm: { durationMs: warm.durationMs, record: warmRecord },
        output: ledger,
      };
      if (runIndex > manifest.policy.warmupRuns) samples.push(sample);
    }
    return samples;
  } finally {
    if (keep) {
      process.stdout.write(`Next ${fixture.id} measurement workspaces retained:\n${runRoots.join("\n")}\n`);
      process.stdout.write(`Next ${fixture.id} validation target retained: ${validationTarget}\n`);
    } else {
      await Promise.all([
        ...runRoots.map((directory) => rm(directory, { recursive: true, force: true })),
        rm(validationTarget, { recursive: true, force: true }),
      ]);
    }
  }
}

export function prepareNextBaselineCli(root) {
  if (process.env.PLIEGO_NEXT_CLI) return path.resolve(process.env.PLIEGO_NEXT_CLI);
  const target = path.join(root, "target", "next-baseline-cli");
  const cli = path.join(target, "release", process.platform === "win32" ? "pliego.exe" : "pliego");
  run("cargo", [
    "build",
    "--manifest-path", path.join(root, "Cargo.toml"),
    "-p", "pliego-cli",
    "--release",
    "--locked",
  ], root, { CARGO_TARGET_DIR: target });
  return cli;
}

function validateRawSample(sample, expectedSample) {
  assert(sample.sample === expectedSample, "fixture sample sequence differs");
  assert(Number.isFinite(sample.cold.durationMs) && sample.cold.durationMs >= 0, "fixture cold duration is invalid");
  assert(Number.isFinite(sample.warm.durationMs) && sample.warm.durationMs >= 0, "fixture warm duration is invalid");
  assert(sample.cold.record.outcome === "executed", "fixture cold build did not execute");
  assert(sample.cold.record.receiptBefore === null, "fixture cold build had prior ownership");
  assert(sample.cold.record.reusedArtifacts === 0, "fixture cold build reused artifacts");
  assert(sample.cold.record.renderedArtifacts > 0, "fixture cold build rendered no artifacts");
  assert(sample.warm.record.outcome === "no-op", "fixture warm build was not a verified no-op");
  assert(sample.warm.record.renderedArtifacts === 0, "fixture warm build rendered artifacts");
  assert(sample.warm.record.reusedArtifacts > 0, "fixture warm build reused no artifacts");
  assert(sample.warm.record.receiptBefore === sample.cold.record.receiptAfter, "fixture warm receiptBefore differs");
  assert(sample.warm.record.receiptAfter === sample.cold.record.receiptAfter, "fixture warm receiptAfter differs");
  assert(sample.output.receiptSha256 === sample.warm.record.receiptAfter, "fixture output receipt differs");
}

function measuredMetric(caseId, samples, select, definitions) {
  const definition = definitions.get(caseId);
  assert(definition, `unknown build metric: ${caseId}`);
  const observations = samples.map((sample) => ({ sample: sample.sample, value: select(sample) }));
  return {
    caseId,
    unit: definition.unit,
    status: "measured",
    observations,
    summary: nearestRankSummary(observations.map((observation) => observation.value)),
  };
}

function validateCold(measurement, record, fixtureId, runIndex) {
  assert(record.outcome === "executed", `${fixtureId} run ${runIndex}: cold build did not execute`);
  assert(record.receiptBefore === null, `${fixtureId} run ${runIndex}: cold build had prior ownership`);
  assert(record.renderedArtifacts > 0 && record.reusedArtifacts === 0, `${fixtureId} run ${runIndex}: cold build counts are invalid`);
  assert(record.phases.totalMicros / 1_000 <= measurement.durationMs, `${fixtureId} run ${runIndex}: cold phases exceed wall time`);
}

function validateWarm(measurement, cold, warm, fixtureId, runIndex) {
  assert(warm.outcome === "no-op", `${fixtureId} run ${runIndex}: warm build was not a no-op`);
  assert(warm.changedSources.length === 0, `${fixtureId} run ${runIndex}: warm build reported changes`);
  assert(warm.renderedArtifacts === 0 && warm.reusedArtifacts > 0, `${fixtureId} run ${runIndex}: warm counts are invalid`);
  assert(warm.receiptBefore === cold.receiptAfter && warm.receiptAfter === cold.receiptAfter, `${fixtureId} run ${runIndex}: warm receipt drifted`);
  assert(recordVisibleNoOp(measurement.stdout), `${fixtureId} run ${runIndex}: no-op was not visible`);
}

async function measured(command, args, cwd, environment) {
  const started = process.hrtime.bigint();
  const result = await spawnMeasured(command, args, cwd, environment);
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed (${result.status})\n${result.stderr || result.stdout}`);
  return {
    durationMs: Number(process.hrtime.bigint() - started) / 1_000_000,
    peakRssBytes: result.peakRssBytes,
    stdout: result.stdout,
  };
}

async function spawnMeasured(command, args, cwd, extraEnvironment) {
  const child = spawnChild(command, args, {
    cwd,
    env: { ...process.env, ...extraEnvironment },
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  const stdout = [];
  const stderr = [];
  let stdoutBytes = 0;
  let stderrBytes = 0;
  let overflow = null;
  const collect = (chunks, stream) => (chunk) => {
    const next = stream === "stdout" ? (stdoutBytes += chunk.length) : (stderrBytes += chunk.length);
    if (next > MAX_PROCESS_OUTPUT_BYTES) {
      overflow = `${stream} exceeded ${MAX_PROCESS_OUTPUT_BYTES} bytes`;
      child.kill();
      return;
    }
    chunks.push(chunk);
  };
  child.stdout.on("data", collect(stdout, "stdout"));
  child.stderr.on("data", collect(stderr, "stderr"));

  const rss = createProcessTreeRssSampler(child.pid);
  const status = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  const peakRssBytes = await rss.stop();
  if (overflow) throw new Error(overflow);
  if (rss.supported) assert(peakRssBytes > 0, "procfs observed no process-tree RSS");
  return {
    status,
    stdout: Buffer.concat(stdout).toString("utf8"),
    stderr: Buffer.concat(stderr).toString("utf8"),
    peakRssBytes,
  };
}

function run(command, args, cwd, extraEnvironment = {}) {
  const result = spawnSyncPortable(command, args, cwd, extraEnvironment);
  if (result.error) throw new Error(`${command} ${args.join(" ")} could not start: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed (${result.status})\n${result.stderr || result.stdout}`);
  return result.stdout;
}

function spawnSyncPortable(command, args, cwd, extraEnvironment) {
  return spawnSync(command, args, {
    cwd,
    env: { ...process.env, ...extraEnvironment },
    encoding: "utf8",
    windowsHide: true,
    maxBuffer: MAX_PROCESS_OUTPUT_BYTES,
    stdio: ["ignore", "pipe", "pipe"],
  });
}

async function pathExists(file) {
  try {
    await stat(file);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function microsToMilliseconds(value) {
  return value / 1_000;
}

function recordVisibleNoOp(stdout) {
  return stdout.includes("[no-op ");
}

function checkedAdd(left, right, message) {
  const value = left + right;
  assert(Number.isSafeInteger(value), message);
  return value;
}

function assertSafeCount(value, label) {
  assert(Number.isSafeInteger(value) && value >= 0, `${label} is not a non-negative safe integer`);
}

function assertPlainObject(value, label) {
  assert(value !== null && typeof value === "object" && !Array.isArray(value), `${label} is not an object`);
}

function assertExactKeys(value, expected, label) {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  assert(JSON.stringify(actual) === JSON.stringify(wanted), `${label} has missing or unexpected fields`);
}

function assertUnique(values, label) {
  assert(new Set(values).size === values.length, `duplicate ${label}`);
}

function assertSorted(values, label) {
  assert(JSON.stringify(values) === JSON.stringify([...values].sort()), `${label} are not sorted`);
}

function nonEmptyString(value) {
  return typeof value === "string" && value.length > 0;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
