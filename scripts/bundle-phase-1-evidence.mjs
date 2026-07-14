import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import {
  copyFile,
  mkdir,
  readFile,
  readdir,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { COLLECTOR_VERSION } from "../tools/route-lab/core.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const defaultRepoRoot = path.resolve(path.dirname(scriptPath), "..");
const evidenceFields = [
  "collectorVersion",
  "tracePath",
  "traceSha256",
  "traceMetricsPath",
  "traceMetricsSha256",
  "screenshotPath",
  "screenshotSha256",
  "rawRunsPath",
];
const metricsFields = [
  "metricsVersion",
  "extractorVersion",
  "sourceTraceSha256",
  "runs",
];
const metricsRunFields = ["sessionId", "decodeDurationsMs", "mainThreadTaskDurationsMs"];
const traceExtensions = [".json.gz", ".json", ".zip"];
const screenshotExtensions = [".png", ".jpg", ".jpeg", ".webp", ".zip"];

export async function bundlePhase1Evidence({
  caseId,
  tracePath,
  screenshotPath,
  metricsPath,
  runsPath,
  collectorVersion = COLLECTOR_VERSION,
  repoRoot = defaultRepoRoot,
}) {
  validateCaseId(caseId);
  if (typeof collectorVersion !== "string" || !collectorVersion.trim()) {
    throw new Error("collectorVersion must be a non-empty string");
  }
  const root = path.resolve(repoRoot);
  const stagedRunsRoot = path.resolve(
    runsPath ?? path.join(root, "measurements", "runs", "staging", caseId),
  );
  const rawRunsRoot = path.join(
    root,
    "measurements",
    "runs",
    "accepted",
    caseId,
  );
  const staged = await loadStagedRuns(stagedRunsRoot);
  const stagedRuns = staged.runs;
  if (staged.collectorVersion !== collectorVersion.trim()) {
    throw new Error(
      `collectorVersion ${collectorVersion.trim()} does not match staged runs ${staged.collectorVersion}`,
    );
  }
  const stagedSessionIds = stagedRuns.map((run) => run.sessionId);
  const trace = await inspectInputArtifact(tracePath, "trace", traceExtensions);
  const screenshot = await inspectInputArtifact(
    screenshotPath,
    "screenshot",
    screenshotExtensions,
  );
  const metrics = await inspectInputArtifact(metricsPath, "trace metrics", [".json"]);
  assertDistinctInputs([trace.resolved, screenshot.resolved, metrics.resolved]);

  const [traceSha256, screenshotSha256] = await Promise.all([
    hashFile(trace.resolved),
    hashFile(screenshot.resolved),
  ]);
  const metricsInput = await readJsonObject(metrics.resolved, "trace metrics");
  const normalizedMetrics = normalizeTraceMetrics(
    metricsInput,
    traceSha256,
    stagedSessionIds,
  );
  const metricsBytes = Buffer.from(`${JSON.stringify(normalizedMetrics, null, 2)}\n`);
  const traceMetricsSha256 = hashBytes(metricsBytes);

  const evidenceRoot = path.join(root, "measurements", "evidence");
  const bundleRoot = path.join(evidenceRoot, caseId);
  const traceName = `trace${trace.extension}`;
  const screenshotName = `screenshot${screenshot.extension}`;
  const traceMetricsName = "trace-metrics.json";
  const evidence = {
    collectorVersion: collectorVersion.trim(),
    tracePath: repoRelative(root, path.join(bundleRoot, traceName)),
    traceSha256,
    traceMetricsPath: repoRelative(root, path.join(bundleRoot, traceMetricsName)),
    traceMetricsSha256,
    screenshotPath: repoRelative(root, path.join(bundleRoot, screenshotName)),
    screenshotSha256,
    rawRunsPath: repoRelative(root, rawRunsRoot),
  };
  const bundle = {
    bundleVersion: "1.0.0",
    caseId,
    state: "complete",
    evidence,
    runs: stagedRuns,
    traceMetrics: {
      metricsVersion: normalizedMetrics.metricsVersion,
      extractorVersion: normalizedMetrics.extractorVersion,
      sourceTraceSha256: normalizedMetrics.sourceTraceSha256,
      runCount: normalizedMetrics.runs.length,
      decodeSampleCount: normalizedMetrics.runs.reduce(
        (total, run) => total + run.decodeDurationsMs.length,
        0,
      ),
      mainThreadTaskSampleCount: normalizedMetrics.runs.reduce(
        (total, run) => total + run.mainThreadTaskDurationsMs.length,
        0,
      ),
    },
  };

  await mkdir(evidenceRoot, { recursive: true });
  if (await exists(bundleRoot)) {
    await verifyExistingBundle({
      bundleRoot,
      bundle,
      evidence,
      traceName,
      screenshotName,
      traceMetricsName,
    });
    return bundle;
  }

  const stageRoot = path.join(
    evidenceRoot,
    `.${caseId}.${process.pid}.${Date.now().toString(36)}.tmp`,
  );
  let published = false;
  await mkdir(stageRoot, { recursive: false });
  try {
    await Promise.all([
      copyFile(trace.resolved, path.join(stageRoot, traceName)),
      copyFile(screenshot.resolved, path.join(stageRoot, screenshotName)),
      writeFile(path.join(stageRoot, traceMetricsName), metricsBytes, { flag: "wx" }),
      writeJsonExclusive(path.join(stageRoot, "evidence.json"), evidence),
      writeJsonExclusive(path.join(stageRoot, "bundle.json"), bundle),
    ]);
    await verifyBundleArtifacts({
      bundleRoot: stageRoot,
      evidence,
      traceName,
      screenshotName,
      traceMetricsName,
    });
    try {
      await rename(stageRoot, bundleRoot);
      published = true;
    } catch (error) {
      if (!(await exists(bundleRoot))) throw error;
      await verifyExistingBundle({
        bundleRoot,
        bundle,
        evidence,
        traceName,
        screenshotName,
        traceMetricsName,
      });
    }
  } finally {
    if (!published) await rm(stageRoot, { recursive: true, force: true });
  }
  return bundle;
}

export function parseCliArguments(argv) {
  const options = {};
  const known = new Set([
    "--case-id",
    "--trace",
    "--screenshot",
    "--metrics",
    "--runs",
    "--collector-version",
  ]);
  if (argv.includes("--help") || argv.includes("-h")) return { help: true };
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!known.has(flag)) throw new Error(`unknown option: ${flag ?? "<missing>"}`);
    if (value === undefined || value.startsWith("--")) {
      throw new Error(`${flag} requires a value`);
    }
    if (Object.hasOwn(options, flag)) throw new Error(`${flag} was provided more than once`);
    options[flag] = value;
  }
  for (const required of ["--case-id", "--trace", "--screenshot", "--metrics"]) {
    if (!options[required]) throw new Error(`${required} is required`);
  }
  return {
    help: false,
    caseId: options["--case-id"],
    tracePath: options["--trace"],
    screenshotPath: options["--screenshot"],
    metricsPath: options["--metrics"],
    runsPath: options["--runs"],
    collectorVersion: options["--collector-version"] ?? COLLECTOR_VERSION,
  };
}

export function normalizeTraceMetrics(value, traceSha256, stagedSessionIds) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("trace metrics must be a JSON object");
  }
  const keys = Object.keys(value).sort();
  if (
    keys.length !== metricsFields.length ||
    keys.some((key, index) => key !== [...metricsFields].sort()[index])
  ) {
    throw new Error(
      `trace metrics must contain exactly: ${metricsFields.join(", ")}`,
    );
  }
  if (value.metricsVersion !== "1.0.0") {
    throw new Error("trace metrics metricsVersion must be 1.0.0");
  }
  if (typeof value.extractorVersion !== "string" || !value.extractorVersion.trim()) {
    throw new Error("trace metrics extractorVersion must be a non-empty string");
  }
  if (!/^[a-f0-9]{64}$/.test(value.sourceTraceSha256 ?? "")) {
    throw new Error("trace metrics sourceTraceSha256 must be a lowercase SHA-256");
  }
  if (value.sourceTraceSha256 !== traceSha256) {
    throw new Error("trace metrics sourceTraceSha256 does not match the trace artifact");
  }
  if (!Array.isArray(stagedSessionIds) || stagedSessionIds.length !== 5) {
    throw new Error("staged raw run set must expose exactly five sessionIds");
  }
  if (!Array.isArray(value.runs) || value.runs.length !== stagedSessionIds.length) {
    throw new Error(
      `trace metrics runs must contain exactly ${stagedSessionIds.length} entries`,
    );
  }
  const seen = new Set();
  const normalizedRuns = value.runs.map((run, index) => {
    if (!run || typeof run !== "object" || Array.isArray(run)) {
      throw new Error(`trace metrics run ${index} must be an object`);
    }
    const runKeys = Object.keys(run).sort();
    const expectedRunKeys = [...metricsRunFields].sort();
    if (
      runKeys.length !== expectedRunKeys.length ||
      runKeys.some((key, keyIndex) => key !== expectedRunKeys[keyIndex])
    ) {
      throw new Error(
        `trace metrics run ${index} must contain exactly: ${metricsRunFields.join(", ")}`,
      );
    }
    if (!/^[a-f0-9]{24}$/.test(run.sessionId ?? "")) {
      throw new Error(`trace metrics run ${index} sessionId is invalid`);
    }
    if (seen.has(run.sessionId)) {
      throw new Error(`trace metrics repeats sessionId ${run.sessionId}`);
    }
    seen.add(run.sessionId);
    validateDurationArray(run.decodeDurationsMs, `runs[${index}].decodeDurationsMs`);
    validateDurationArray(
      run.mainThreadTaskDurationsMs,
      `runs[${index}].mainThreadTaskDurationsMs`,
    );
    return {
      sessionId: run.sessionId,
      decodeDurationsMs: [...run.decodeDurationsMs],
      mainThreadTaskDurationsMs: [...run.mainThreadTaskDurationsMs],
    };
  });
  const expectedIds = [...stagedSessionIds].sort();
  const actualIds = [...seen].sort();
  if (actualIds.some((sessionId, index) => sessionId !== expectedIds[index])) {
    throw new Error("trace metrics sessionIds do not match the staged raw run set");
  }
  return {
    metricsVersion: value.metricsVersion,
    extractorVersion: value.extractorVersion.trim(),
    sourceTraceSha256: value.sourceTraceSha256,
    runs: normalizedRuns.sort((left, right) => left.sessionId.localeCompare(right.sessionId)),
  };
}

function validateDurationArray(value, name) {
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error(`trace metrics ${name} must contain at least one sample`);
  }
  if (value.length > 20_000) {
    throw new Error(`trace metrics ${name} must contain at most 20000 samples`);
  }
  if (value.some((sample) => !Number.isFinite(sample) || sample < 0)) {
    throw new Error(`trace metrics ${name} must contain only finite non-negative numbers`);
  }
}

async function loadStagedRuns(stagedRunsRoot) {
  let entries;
  try {
    entries = await stat(stagedRunsRoot);
  } catch (error) {
    if (error.code === "ENOENT") {
      throw new Error(`staged raw run directory is missing: ${stagedRunsRoot}`);
    }
    throw error;
  }
  if (!entries.isDirectory()) {
    throw new Error("staged raw run path must be a directory");
  }
  const fileNames = (await readdir(stagedRunsRoot))
    .filter((fileName) => fileName.endsWith(".json"))
    .sort();
  if (fileNames.length !== 5) {
    throw new Error(
      `staged raw run directory contains ${fileNames.length} JSON files; expected 5`,
    );
  }
  const runs = [];
  const collectorVersions = new Set();
  for (const fileName of fileNames) {
    const filePath = path.join(stagedRunsRoot, fileName);
    const run = await readJsonObject(filePath, `staged run ${fileName}`);
    if (!/^[a-f0-9]{24}$/.test(run.sessionId ?? "")) {
      throw new Error(`staged run ${fileName} has an invalid sessionId`);
    }
    if (typeof run.server?.collectorVersion !== "string" || !run.server.collectorVersion.trim()) {
      throw new Error(`staged run ${fileName} has an invalid collectorVersion`);
    }
    collectorVersions.add(run.server.collectorVersion.trim());
    runs.push({
      fileName,
      sessionId: run.sessionId,
      sha256: await hashFile(filePath),
    });
  }
  if (new Set(runs.map((run) => run.sessionId)).size !== runs.length) {
    throw new Error("staged raw run directory repeats a sessionId");
  }
  if (collectorVersions.size !== 1) {
    throw new Error("staged raw run directory mixes collector versions");
  }
  return { runs, collectorVersion: [...collectorVersions][0] };
}

function validateCaseId(value) {
  if (
    typeof value !== "string" ||
    !/^[a-z0-9](?:[a-z0-9_-]{0,126}[a-z0-9])?$/.test(value)
  ) {
    throw new Error(
      "caseId must be 1-128 lowercase letters, numbers, hyphens, or underscores and end in a letter or number",
    );
  }
  if (/^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])$/i.test(value)) {
    throw new Error("caseId is reserved by Windows");
  }
}

async function inspectInputArtifact(inputPath, label, extensions) {
  if (typeof inputPath !== "string" || !inputPath.trim()) {
    throw new Error(`${label} path is required`);
  }
  const resolved = path.resolve(inputPath);
  let stats;
  try {
    stats = await stat(resolved);
  } catch (error) {
    if (error.code === "ENOENT") throw new Error(`${label} artifact is missing: ${inputPath}`);
    throw error;
  }
  if (!stats.isFile()) throw new Error(`${label} artifact must be one file`);
  if (stats.size === 0) throw new Error(`${label} artifact must not be empty`);
  const extension = extensions.find((candidate) =>
    path.basename(resolved).toLowerCase().endsWith(candidate),
  );
  if (!extension) {
    throw new Error(`${label} artifact has an unsupported extension`);
  }
  return { resolved, extension };
}

function assertDistinctInputs(paths) {
  const normalized = paths.map((value) => path.resolve(value).toLowerCase());
  if (new Set(normalized).size !== normalized.length) {
    throw new Error("trace, screenshot, and trace metrics must be distinct files");
  }
}

async function readJsonObject(filePath, label) {
  try {
    return JSON.parse(await readFile(filePath, "utf8"));
  } catch (error) {
    if (error instanceof SyntaxError) throw new Error(`${label} artifact is not valid JSON`);
    throw error;
  }
}

async function verifyExistingBundle({
  bundleRoot,
  bundle,
  evidence,
  traceName,
  screenshotName,
  traceMetricsName,
}) {
  let existingBundle;
  let existingEvidence;
  try {
    [existingBundle, existingEvidence] = await Promise.all([
      readJsonObject(path.join(bundleRoot, "bundle.json"), "existing bundle"),
      readJsonObject(path.join(bundleRoot, "evidence.json"), "existing evidence"),
    ]);
  } catch (error) {
    throw new Error(`evidence bundle already exists but is incomplete: ${error.message}`);
  }
  if (
    JSON.stringify(existingBundle) !== JSON.stringify(bundle) ||
    JSON.stringify(existingEvidence) !== JSON.stringify(evidence)
  ) {
    throw new Error("evidence bundle already exists with different immutable metadata");
  }
  await verifyBundleArtifacts({
    bundleRoot,
    evidence,
    traceName,
    screenshotName,
    traceMetricsName,
  });
}

async function verifyBundleArtifacts({
  bundleRoot,
  evidence,
  traceName,
  screenshotName,
  traceMetricsName,
}) {
  const [traceSha256, screenshotSha256, traceMetricsSha256] = await Promise.all([
    hashFile(path.join(bundleRoot, traceName)),
    hashFile(path.join(bundleRoot, screenshotName)),
    hashFile(path.join(bundleRoot, traceMetricsName)),
  ]);
  if (traceSha256 !== evidence.traceSha256) {
    throw new Error("copied trace SHA-256 does not match bundle metadata");
  }
  if (screenshotSha256 !== evidence.screenshotSha256) {
    throw new Error("copied screenshot SHA-256 does not match bundle metadata");
  }
  if (traceMetricsSha256 !== evidence.traceMetricsSha256) {
    throw new Error("canonical trace metrics SHA-256 does not match bundle metadata");
  }
}

async function writeJsonExclusive(filePath, value) {
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, { flag: "wx" });
}

async function hashFile(filePath) {
  const hash = createHash("sha256");
  await new Promise((resolve, reject) => {
    const stream = createReadStream(filePath);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("error", reject);
    stream.on("end", resolve);
  });
  return hash.digest("hex");
}

function hashBytes(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function repoRelative(repoRoot, absolutePath) {
  const relative = path.relative(repoRoot, absolutePath);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error("canonical evidence path escapes the repository");
  }
  return relative.split(path.sep).join("/");
}

async function exists(filePath) {
  try {
    await stat(filePath);
    return true;
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

function usage() {
  return [
    "Usage:",
    "  npm run bundle:phase-1-evidence -- --case-id <id> --trace <trace.json|json.gz|zip> --screenshot <image> --metrics <trace-metrics.json> [--runs <staging-dir>]",
    "",
    "Options:",
    `  --collector-version <version>  Defaults to ${COLLECTOR_VERSION}`,
    "  --runs <directory>             Defaults to measurements/runs/staging/<case-id>",
    "  --help                         Show this help",
  ].join("\n");
}

async function main() {
  try {
    const options = parseCliArguments(process.argv.slice(2));
    if (options.help) {
      process.stdout.write(`${usage()}\n`);
      return;
    }
    const bundle = await bundlePhase1Evidence(options);
    process.stdout.write(`${JSON.stringify(bundle, null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`Evidence bundle failed: ${error.message}\n\n${usage()}\n`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) await main();

export { evidenceFields, metricsFields, metricsRunFields };
