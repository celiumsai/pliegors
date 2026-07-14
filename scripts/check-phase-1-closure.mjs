import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import {
  enumerateRequiredCases,
  evaluateBudgets,
  measurementCaseKey,
} from "./phase-1-coverage.mjs";
import { auditAcceptedEvidence } from "./phase-1-evidence.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const plan = await readJson("fixtures/phase-1/measurement-plan.json");
const baseline = await readJson("fixtures/phase-1/baseline.expected.json");
const waiverDocument = await readJson("fixtures/phase-1/budget-waivers.json");
const reportSchema = await readJson("schemas/pliego.measurement-report.schema.json");
const runSchema = await readJson("schemas/pliego.measurement-run.schema.json");
const traceMetricsSchema = await readJson("schemas/pliego.trace-metrics.schema.json");
const acceptedFingerprintRoot = path.join(repoRoot, "measurements", "accepted");
const acceptedReportRoot = path.join(repoRoot, "measurements", "reports", "accepted");
const acceptedRunRoot = path.join(repoRoot, "measurements", "runs", "accepted");
const format = process.argv.includes("--format")
  ? process.argv[process.argv.indexOf("--format") + 1]
  : "human";
if (!new Set(["human", "json"]).has(format)) {
  throw new Error("--format must be human or json");
}

const ajv = new Ajv2020({ allErrors: true, allowUnionTypes: true, strict: true });
addFormats(ajv);
const validateReport = ajv.compile(reportSchema);
const validateRun = ajv.compile(runSchema);
const validateTraceMetrics = ajv.compile(traceMetricsSchema);
const fingerprints = await loadJsonFiles(acceptedFingerprintRoot);
const reports = await loadJsonFiles(acceptedReportRoot);
const requiredDevices = plan.devices.filter((device) => device.required);
const deviceBlockers = [];
const budgetBlockers = [];

const failedBudgets = baseline.targets.flatMap((target) =>
  target.budgetResults
    .filter((budget) => !budget.passed)
    .map((budget) => ({ targetId: target.work.id, budgetId: budget.id })),
);
const waiverKeys = new Set(
  waiverDocument.waivers.map((waiver) => `${waiver.targetId}|${waiver.budgetId}`),
);
for (const budget of failedBudgets) {
  const key = `${budget.targetId}|${budget.budgetId}`;
  if (!waiverKeys.has(key)) budgetBlockers.push(`${key}: failed without a Phase 1 waiver`);
}
for (const key of waiverKeys) {
  if (!failedBudgets.some((budget) => `${budget.targetId}|${budget.budgetId}` === key)) {
    budgetBlockers.push(`${key}: stale waiver without matching debt`);
  }
}

for (const device of requiredDevices) {
  if (device.status !== "ready") {
    deviceBlockers.push(`${device.id}: status is ${device.status}`);
  }
  if (!fingerprints.some(({ value }) => value.deviceId === device.id)) {
    deviceBlockers.push(`${device.id}: accepted fingerprint missing`);
  }
}

const reportErrors = [];
const reportByCase = new Map();
const validReports = [];
for (const { fileName, value } of reports) {
  if (!validateReport(value)) {
    reportErrors.push(
      `${fileName}: ${ajv.errorsText(validateReport.errors, { separator: "; " })}`,
    );
    continue;
  }
  validReports.push({ fileName, value });
  for (const failure of evaluateBudgets(value, plan.budgets)) {
    reportErrors.push(`${fileName}: budget ${failure}`);
  }
}

const acceptedEvidence = await auditAcceptedEvidence({
  repoRoot,
  acceptedRunRoot,
  acceptedFingerprintRoot,
  reports: validReports,
  measuredRuns: plan.policy.measuredRuns,
  validateRun,
  validateTraceMetrics,
  formatRunErrors: (errors) => ajv.errorsText(errors, { separator: "; " }),
  formatTraceMetricsErrors: (errors) => ajv.errorsText(errors, { separator: "; " }),
});
reportErrors.push(...acceptedEvidence.errors);
for (const report of acceptedEvidence.reviews.filter((review) => review.accepted)) {
  const key = measurementCaseKey(report.value);
  if (reportByCase.has(key)) reportErrors.push(`${report.fileName}: duplicate case ${key}`);
  else reportByCase.set(key, { fileName: report.fileName, value: report.value });
}

const requiredCases = enumerateRequiredCases(plan);
const requiredKeys = new Set(requiredCases.map(measurementCaseKey));
const missingCases = requiredCases.filter((item) => !reportByCase.has(measurementCaseKey(item)));
for (const [key, report] of reportByCase) {
  if (!requiredKeys.has(key)) reportErrors.push(`${report.fileName}: unplanned case ${key}`);
}

const result = {
  status:
    deviceBlockers.length === 0 &&
    budgetBlockers.length === 0 &&
    missingCases.length === 0 &&
    reportErrors.length === 0
      ? "complete"
      : "blocked",
  requiredDeviceRows: requiredDevices.length,
  readyDeviceRows: requiredDevices.filter((device) => device.status === "ready").length,
  acceptedFingerprints: fingerprints.length,
  activeBudgetWaivers: waiverDocument.waivers.length,
  requiredReports: requiredCases.length,
  acceptedReports: reportByCase.size,
  missingReports: missingCases.length,
  deviceBlockers,
  budgetBlockers,
  reportErrors,
  missingCaseSample: missingCases.slice(0, 12),
};

if (format === "json") {
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
} else {
  process.stdout.write(
    [
      `Phase 1 closure: ${result.status.toUpperCase()}`,
      `device rows: ${result.readyDeviceRows}/${result.requiredDeviceRows} ready`,
      `accepted fingerprints: ${result.acceptedFingerprints}`,
      `active Phase 1 budget waivers: ${result.activeBudgetWaivers}`,
      `aggregate reports: ${result.acceptedReports}/${result.requiredReports}`,
      `missing reports: ${result.missingReports}`,
      ...deviceBlockers.map((value) => `device blocker: ${value}`),
      ...budgetBlockers.map((value) => `budget blocker: ${value}`),
      ...reportErrors.slice(0, 12).map((value) => `report error: ${value}`),
    ].join("\n") + "\n",
  );
}

if (result.status !== "complete") process.exitCode = 1;

async function readJson(relativePath) {
  return JSON.parse(await readFile(path.join(repoRoot, relativePath), "utf8"));
}

async function loadJsonFiles(directory) {
  try {
    const fileNames = (await readdir(directory))
      .filter((fileName) => fileName.endsWith(".json"))
      .sort();
    return await Promise.all(
      fileNames.map(async (fileName) => ({
        fileName,
        value: JSON.parse(await readFile(path.join(directory, fileName), "utf8")),
      })),
    );
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }
}
