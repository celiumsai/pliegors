import { createHash } from "node:crypto";
import { readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";
import { validateInitialRouteLedger } from "../tools/route-lab/core.mjs";
import {
  aggregateAcceptedRuns,
  measurementCaseKey,
  percentile,
} from "./phase-1-coverage.mjs";

export async function auditAcceptedEvidence({
  repoRoot,
  acceptedRunRoot,
  acceptedFingerprintRoot,
  reports,
  measuredRuns,
  validateRun,
  validateTraceMetrics,
  formatRunErrors = () => "schema validation failed",
  formatTraceMetricsErrors = () => "schema validation failed",
}) {
  const reviews = [];
  const referencedDirectories = new Set();

  for (const reportRecord of reports) {
    const review = await reviewAcceptedReport({
      repoRoot,
      acceptedRunRoot,
      acceptedFingerprintRoot,
      report: reportRecord.value,
      measuredRuns,
      validateRun,
      validateTraceMetrics,
      formatRunErrors,
      formatTraceMetricsErrors,
    });
    reviews.push({ ...reportRecord, ...review });
    if (review.rawRunsDirectory) referencedDirectories.add(review.rawRunsDirectory);
  }

  const directoryOwners = new Map();
  const caseOwners = new Map();
  for (const review of reviews) {
    if (!review.rawRunsDirectory) continue;
    const owner = directoryOwners.get(review.rawRunsDirectory);
    if (owner) {
      review.errors.push(`rawRunsPath is already referenced by ${owner}`);
    } else {
      directoryOwners.set(review.rawRunsDirectory, review.fileName);
    }
    const caseKey = measurementCaseKey(review.value);
    const caseOwner = caseOwners.get(caseKey);
    if (caseOwner) {
      review.errors.push(`measurement case is already reported by ${caseOwner}`);
    } else {
      caseOwners.set(caseKey, review.fileName);
    }
  }

  const layoutErrors = await validateAcceptedRunLayout(
    acceptedRunRoot,
    referencedDirectories,
  );
  const errors = [
    ...layoutErrors.map((error) => `accepted runs: ${error}`),
    ...reviews.flatMap((review) =>
      review.errors.map((error) => `${review.fileName}: ${error}`),
    ),
  ];
  const acceptedReviews = reviews.filter((review) => review.errors.length === 0);

  return {
    errors,
    reviews: reviews.map((review) => ({
      ...review,
      accepted: review.errors.length === 0,
    })),
    acceptedReportCount: acceptedReviews.length,
    acceptedRunCount: acceptedReviews.reduce(
      (total, review) => total + review.runCount,
      0,
    ),
  };
}

async function reviewAcceptedReport({
  repoRoot,
  acceptedRunRoot,
  acceptedFingerprintRoot,
  report,
  measuredRuns,
  validateRun,
  validateTraceMetrics,
  formatRunErrors,
  formatTraceMetricsErrors,
}) {
  const errors = [];
  const traceError = await checkArtifact(
    repoRoot,
    report.evidence.tracePath,
    report.evidence.traceSha256,
    "trace",
  );
  if (traceError) errors.push(`trace ${traceError}`);
  const screenshotError = await checkArtifact(
    repoRoot,
    report.evidence.screenshotPath,
    report.evidence.screenshotSha256,
    "screenshot",
  );
  if (screenshotError) errors.push(`screenshot ${screenshotError}`);

  const runSet = await validateRunSet({
    repoRoot,
    acceptedRunRoot,
    acceptedFingerprintRoot,
    relativePath: report.evidence.rawRunsPath,
    report,
    measuredRuns,
    validateRun,
    formatRunErrors,
  });
  errors.push(...runSet.errors.map((error) => `raw runs ${error}`));

  const traceMetrics = await loadTraceMetrics({
    repoRoot,
    report,
    sessionIds: runSet.sessionIds,
    measuredRuns,
    validateTraceMetrics,
    formatTraceMetricsErrors,
  });
  errors.push(...traceMetrics.errors.map((error) => `trace metrics ${error}`));

  if (
    runSet.runs.length === report.runCount &&
    traceMetrics.metricsBySession.size === report.runCount
  ) {
    try {
      const runsWithTraceMetrics = runSet.runs.map((run) => {
        const metrics = traceMetrics.metricsBySession.get(run.sessionId);
        return {
          ...run,
          observations: {
            ...run.observations,
            decodeP95Ms: percentile(metrics.decodeDurationsMs, 0.95),
            mainThreadP95Ms: percentile(metrics.mainThreadTaskDurationsMs, 0.95),
          },
        };
      });
      const calculated = aggregateAcceptedRuns(runsWithTraceMetrics);
      for (const [metric, value] of Object.entries(calculated)) {
        const reportedValue = report.observations[metric];
        if (!Number.isFinite(reportedValue) || Math.abs(value - reportedValue) > 0.01) {
          errors.push(
            `aggregate ${metric} is ${reportedValue}, recalculated value is ${value}`,
          );
        }
      }
    } catch (error) {
      errors.push(`aggregate cannot be recalculated: ${error.message}`);
    }
  }

  return {
    errors,
    rawRunsDirectory: runSet.rawRunsDirectory,
    runCount: runSet.runCount,
  };
}

async function validateRunSet({
  repoRoot,
  acceptedRunRoot,
  acceptedFingerprintRoot,
  relativePath,
  report,
  measuredRuns,
  validateRun,
  formatRunErrors,
}) {
  const errors = [];
  const result = await resolveRepositoryPath(repoRoot, relativePath);
  if (result.error) {
    return {
      errors: [result.error],
      rawRunsDirectory: null,
      runCount: 0,
      runs: [],
      sessionIds: [],
    };
  }
  if (!result.stats.isDirectory()) {
    return {
      errors: ["rawRunsPath must be a directory"],
      rawRunsDirectory: result.resolved,
      runCount: 0,
      runs: [],
      sessionIds: [],
    };
  }

  const relativeToAccepted = path.relative(acceptedRunRoot, result.resolved);
  if (
    !relativeToAccepted ||
    relativeToAccepted.startsWith("..") ||
    path.isAbsolute(relativeToAccepted) ||
    path.dirname(relativeToAccepted) !== "."
  ) {
    return {
      errors: [
        "rawRunsPath must name one case directory directly under measurements/runs/accepted",
      ],
      rawRunsDirectory: result.resolved,
      runCount: 0,
      runs: [],
      sessionIds: [],
    };
  }

  const rawRunsDirectory = path.resolve(result.resolved);
  const fileNames = (await readdir(rawRunsDirectory))
    .filter((fileName) => fileName.endsWith(".json"))
    .sort();
  if (report.runCount !== measuredRuns) {
    errors.push(`runCount must equal policy measuredRuns (${measuredRuns})`);
  }
  if (fileNames.length !== report.runCount) {
    errors.push(`contains ${fileNames.length} JSON runs, expected ${report.runCount}`);
  }

  const fingerprintError = await validateFingerprintBinding(
    acceptedFingerprintRoot,
    report.hardwareFingerprint,
  );
  if (fingerprintError) errors.push(fingerprintError);
  if (!/^html-sha256:[a-f0-9]{64};manifest-sha256:[a-f0-9]{64}$/.test(report.sourceRevision)) {
    errors.push("source revision is not sealed with HTML and manifest hashes");
  }

  const runs = [];
  const sessionIds = new Set();
  for (const fileName of fileNames) {
    let run;
    try {
      run = JSON.parse(await readFile(path.join(rawRunsDirectory, fileName), "utf8"));
    } catch (error) {
      errors.push(`${fileName} is not valid JSON: ${error.message}`);
      continue;
    }
    if (!validateRun(run)) {
      errors.push(`${fileName} fails schema: ${formatRunErrors(validateRun.errors)}`);
      continue;
    }
    if (run.violations.length) errors.push(`${fileName} contains violations`);
    for (const ledgerError of validateInitialRouteLedger(run)) {
      errors.push(`${fileName} resource ledger ${ledgerError}`);
    }
    if (
      measurementCaseKey({ ...run, orientation: run.viewport.orientation }) !==
      measurementCaseKey(report)
    ) {
      errors.push(`${fileName} belongs to a different measurement case`);
    }
    if (run.hardwareFingerprint !== report.hardwareFingerprint) {
      errors.push(`${fileName} hardware fingerprint differs from report`);
    }
    if (run.browserFingerprint !== report.browserFingerprint) {
      errors.push(`${fileName} browser fingerprint differs from report`);
    }
    if (run.sourceRevision !== report.sourceRevision) {
      errors.push(`${fileName} source revision differs from report`);
    }
    if (run.server.collectorVersion !== report.evidence.collectorVersion) {
      errors.push(`${fileName} collector version differs from report evidence`);
    }
    if (sessionIds.has(run.sessionId)) errors.push(`${fileName} repeats sessionId`);
    sessionIds.add(run.sessionId);
    const withoutReceipt = { ...run };
    delete withoutReceipt.server;
    const receipt = createHash("sha256")
      .update(`${JSON.stringify(withoutReceipt)}\n`)
      .digest("hex");
    if (receipt !== run.server.sha256) errors.push(`${fileName} receipt mismatch`);
    runs.push(run);
  }

  return {
    errors,
    rawRunsDirectory,
    runCount: fileNames.length,
    runs,
    sessionIds: [...sessionIds],
  };
}

async function loadTraceMetrics({
  repoRoot,
  report,
  sessionIds,
  measuredRuns,
  validateTraceMetrics,
  formatTraceMetricsErrors,
}) {
  const errors = [];
  const metricsBySession = new Map();
  const relativePath = report.evidence.traceMetricsPath;
  const artifactError = await checkArtifact(
    repoRoot,
    relativePath,
    report.evidence.traceMetricsSha256,
    "trace-metrics",
  );
  if (artifactError) return { errors: [artifactError], metricsBySession };

  let metrics;
  try {
    metrics = JSON.parse(await readFile(path.resolve(repoRoot, relativePath), "utf8"));
  } catch (error) {
    return { errors: [`cannot be parsed as JSON: ${error.message}`], metricsBySession };
  }
  if (validateTraceMetrics && !validateTraceMetrics(metrics)) {
    errors.push(`fails schema: ${formatTraceMetricsErrors(validateTraceMetrics.errors)}`);
    return { errors, metricsBySession };
  }
  if (!isTraceMetrics(metrics)) {
    errors.push("does not match the required trace-metrics structure");
    return { errors, metricsBySession };
  }
  if (metrics.sourceTraceSha256 !== report.evidence.traceSha256) {
    errors.push("sourceTraceSha256 does not match the report trace hash");
  }
  if (metrics.runs.length !== measuredRuns) {
    errors.push(`contains ${metrics.runs.length} run metrics, expected ${measuredRuns}`);
  }
  for (const runMetrics of metrics.runs) {
    if (metricsBySession.has(runMetrics.sessionId)) {
      errors.push(`repeats sessionId ${runMetrics.sessionId}`);
    } else {
      metricsBySession.set(runMetrics.sessionId, runMetrics);
    }
  }
  const rawSessionIds = new Set(sessionIds);
  for (const sessionId of rawSessionIds) {
    if (!metricsBySession.has(sessionId)) {
      errors.push(`is missing raw-run sessionId ${sessionId}`);
    }
  }
  for (const sessionId of metricsBySession.keys()) {
    if (!rawSessionIds.has(sessionId)) {
      errors.push(`contains foreign sessionId ${sessionId}`);
    }
  }
  return { errors, metricsBySession };
}

function isTraceMetrics(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value).sort();
  if (
    keys.join("|") !==
    ["extractorVersion", "metricsVersion", "runs", "sourceTraceSha256"].sort().join("|")
  ) {
    return false;
  }
  if (
    value.metricsVersion !== "1.0.0" ||
    typeof value.extractorVersion !== "string" ||
    !value.extractorVersion.trim() ||
    !/^[a-f0-9]{64}$/.test(value.sourceTraceSha256) ||
    !Array.isArray(value.runs)
  ) {
    return false;
  }
  return value.runs.every((run) => {
    if (!run || typeof run !== "object" || Array.isArray(run)) return false;
    const runKeys = Object.keys(run).sort();
    if (
      runKeys.join("|") !==
      ["decodeDurationsMs", "mainThreadTaskDurationsMs", "sessionId"].sort().join("|")
    ) {
      return false;
    }
    return (
      typeof run.sessionId === "string" &&
      /^[a-f0-9]{24}$/.test(run.sessionId) &&
      validDurationArray(run.decodeDurationsMs) &&
      validDurationArray(run.mainThreadTaskDurationsMs)
    );
  });
}

function validDurationArray(values) {
  return (
    Array.isArray(values) &&
    values.length > 0 &&
    values.length <= 20_000 &&
    values.every((value) => Number.isFinite(value) && value >= 0)
  );
}

async function validateAcceptedRunLayout(acceptedRunRoot, referencedDirectories) {
  let entries;
  try {
    entries = await readdir(acceptedRunRoot, { withFileTypes: true });
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }

  const errors = [];
  for (const entry of entries) {
    if (entry.isFile() && entry.name.endsWith(".json")) {
      errors.push(`${entry.name} is a loose JSON; accepted runs must live in case directories`);
      continue;
    }
    if (!entry.isDirectory()) continue;
    const directory = path.resolve(acceptedRunRoot, entry.name);
    if (referencedDirectories.has(directory)) continue;
    const jsonFiles = (await readdir(directory)).filter((fileName) => fileName.endsWith(".json"));
    if (jsonFiles.length) {
      errors.push(`${entry.name} is an unreferenced accepted-run case directory`);
    }
  }
  return errors;
}

async function checkArtifact(repoRoot, relativePath, expectedSha256, kind) {
  if (typeof relativePath !== "string" || !relativePath) return "path is missing";
  if (!/^[a-f0-9]{64}$/.test(expectedSha256)) return "SHA-256 is missing or malformed";
  const result = await resolveRepositoryPath(repoRoot, relativePath);
  if (result.error) return result.error;
  if (!result.stats.isFile()) return "must be one immutable file";
  const allowed =
    kind === "trace"
      ? /\.(?:json|json\.gz|zip)$/i
      : kind === "trace-metrics"
        ? /\.json$/i
        : /\.(?:png|jpe?g|webp|zip)$/i;
  if (!allowed.test(relativePath)) return "has an unsupported artifact extension";
  const bytes = await readFile(result.resolved);
  const actual = createHash("sha256").update(bytes).digest("hex");
  return actual === expectedSha256 ? null : "SHA-256 does not match the report";
}

async function validateFingerprintBinding(acceptedFingerprintRoot, binding) {
  const match = binding.match(/^(.+\.json)#sha256=([a-f0-9]{64})$/);
  if (!match) return "hardware fingerprint lacks a hashed accepted-fingerprint binding";
  try {
    const bytes = await readFile(path.join(acceptedFingerprintRoot, path.basename(match[1])));
    const actual = createHash("sha256").update(bytes).digest("hex");
    return actual === match[2]
      ? null
      : "hardware fingerprint hash does not match the accepted fingerprint";
  } catch (error) {
    if (error.code === "ENOENT") return "accepted hardware fingerprint is missing";
    throw error;
  }
}

async function resolveRepositoryPath(repoRoot, relativePath) {
  const resolved = path.resolve(repoRoot, relativePath);
  const relative = path.relative(repoRoot, resolved);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    return { error: "escapes the repository" };
  }
  try {
    return { resolved, stats: await stat(resolved) };
  } catch (error) {
    if (error.code === "ENOENT") return { error: `is missing: ${relativePath}` };
    throw error;
  }
}
