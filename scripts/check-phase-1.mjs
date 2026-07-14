import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import { auditAcceptedEvidence } from "./phase-1-evidence.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const baselinePath = path.join(
  repoRoot,
  "fixtures",
  "phase-1",
  "baseline.expected.json",
);
const targetPath = path.join(repoRoot, "fixtures", "targets.json");

run("cargo", ["fmt", "--all", "--", "--check"]);
run("cargo", [
  "clippy",
  "-p",
  "pliego-inspect",
  "--all-targets",
  "--",
  "-D",
  "warnings",
]);
run("cargo", ["test", "-p", "pliego-inspect"]);

const schemaPaths = [
  "schemas/pliego.budget-waivers.schema.json",
  "schemas/pliego.asset-manifest.schema.json",
  "schemas/pliego.device-fingerprint.schema.json",
  "schemas/pliego.measurement-plan.schema.json",
  "schemas/pliego.measurement-report.schema.json",
  "schemas/pliego.measurement-run.schema.json",
  "schemas/pliego.trace-metrics.schema.json",
];
const documentPaths = [
  "fixtures/phase-1/budget-waivers.json",
  "fixtures/phase-1/measurement-plan.json",
  "fixtures/targets.json",
  "fixtures/manifests/pliegors-site.asset-manifest.json",
];
for (const relativePath of [...schemaPaths, ...documentPaths]) {
  JSON.parse(await readFile(path.join(repoRoot, relativePath), "utf8"));
}

const ajv = new Ajv2020({ allErrors: true, allowUnionTypes: true, strict: true });
addFormats(ajv);
for (const relativePath of schemaPaths) {
  ajv.addSchema(JSON.parse(await readFile(path.join(repoRoot, relativePath), "utf8")));
}

const measurementPlan = await validateFile(
  "https://pliegors.dev/schemas/pliego.measurement-plan.schema.json",
  "fixtures/phase-1/measurement-plan.json",
);
await validateFile(
  "https://pliegors.dev/schemas/pliego.budget-waivers.schema.json",
  "fixtures/phase-1/budget-waivers.json",
);
for (const relativePath of ["fixtures/manifests/pliegors-site.asset-manifest.json"]) {
  await validateFile("https://pliegors.dev/schemas/pliego.asset-manifest.schema.json", relativePath);
}

const acceptedRoot = path.join(repoRoot, "measurements", "accepted");
const acceptedFiles = (await readdir(acceptedRoot))
  .filter((fileName) => fileName.endsWith(".json"))
  .sort();
for (const fileName of acceptedFiles) {
  const fingerprint = await validateFile(
    "https://pliegors.dev/schemas/pliego.device-fingerprint.schema.json",
    path.join("measurements", "accepted", fileName),
  );
  if (!fingerprint.webgl?.renderer) {
    throw new Error(`Accepted fingerprint has no WebGL renderer: ${fileName}`);
  }
  if (
    fingerprint.frameProbe.medianMs > fingerprint.frameProbe.p95Ms ||
    fingerprint.frameProbe.p95Ms > fingerprint.frameProbe.maxMs
  ) {
    throw new Error(`Accepted fingerprint has inconsistent frame percentiles: ${fileName}`);
  }
}

const acceptedReportFiles = await listJson(path.join(repoRoot, "measurements", "reports", "accepted"));
const acceptedReports = [];
for (const fileName of acceptedReportFiles) {
  const value = await validateFile(
    "https://pliegors.dev/schemas/pliego.measurement-report.schema.json",
    path.join("measurements", "reports", "accepted", fileName),
  );
  acceptedReports.push({ fileName, value });
}
const runValidator = ajv.getSchema(
  "https://pliegors.dev/schemas/pliego.measurement-run.schema.json",
);
const traceMetricsValidator = ajv.getSchema(
  "https://pliegors.dev/schemas/pliego.trace-metrics.schema.json",
);
const acceptedEvidence = await auditAcceptedEvidence({
  repoRoot,
  acceptedRunRoot: path.join(repoRoot, "measurements", "runs", "accepted"),
  acceptedFingerprintRoot: acceptedRoot,
  reports: acceptedReports,
  measuredRuns: measurementPlan.policy.measuredRuns,
  validateRun: runValidator,
  validateTraceMetrics: traceMetricsValidator,
  formatRunErrors: (errors) => ajv.errorsText(errors, { separator: "; " }),
  formatTraceMetricsErrors: (errors) => ajv.errorsText(errors, { separator: "; " }),
});
if (acceptedEvidence.errors.length) {
  throw new Error(`Accepted measurement evidence is invalid:\n${acceptedEvidence.errors.join("\n")}`);
}

const rejectedRoot = path.join(repoRoot, "measurements", "rejected");
const rejectedFiles = (await readdir(rejectedRoot))
  .filter((fileName) => fileName.endsWith(".json"))
  .sort();
for (const fileName of rejectedFiles) {
  JSON.parse(await readFile(path.join(rejectedRoot, fileName), "utf8"));
  const reasonPath = path.join(
    rejectedRoot,
    `${fileName.slice(0, -".json".length)}.reason.md`,
  );
  const reason = await readFile(reasonPath, "utf8");
  if (!reason.includes("Reason:") || !reason.includes("Action:")) {
    throw new Error(`Rejected fingerprint lacks a complete reason: ${fileName}`);
  }
}

const generated = run(
  "cargo",
  [
    "run",
    "-q",
    "-p",
    "pliego-inspect",
    "--",
    "baseline",
    targetPath,
    "--format",
    "json",
  ],
  true,
);
const expected = await readFile(baselinePath);
if (!generated.equals(expected)) {
  process.stderr.write(
    "Phase 1 baseline drifted. Regenerate baseline.expected.json and review the diff.\n",
  );
  process.exit(1);
}

const baseline = JSON.parse(expected.toString("utf8"));
if (!baseline.valid || baseline.targetCount !== 1) {
  throw new Error("Phase 1 baseline is invalid or does not contain all fixtures");
}
const digest = createHash("sha256").update(expected).digest("hex");
process.stdout.write(
  [
    "Phase 1 deterministic gate: PASS",
    `targets: ${baseline.targetCount}`,
    `assets: ${baseline.assetCount}`,
    `variants: ${baseline.variantCount}`,
    `known budget debt: ${baseline.failedBudgetCount}`,
    `accepted fingerprints: ${acceptedFiles.length}`,
    `accepted route runs: ${acceptedEvidence.acceptedRunCount}`,
    `accepted reports: ${acceptedEvidence.acceptedReportCount}`,
    `rejected fingerprints: ${rejectedFiles.length}`,
    `baseline sha256: ${digest}`,
  ].join(" | ") + "\n",
);

async function validateFile(schemaId, relativePath) {
  const validate = ajv.getSchema(schemaId);
  if (!validate) throw new Error(`Schema is not registered: ${schemaId}`);
  const value = JSON.parse(await readFile(path.join(repoRoot, relativePath), "utf8"));
  if (!validate(value)) {
    const details = ajv.errorsText(validate.errors, { separator: "\n" });
    throw new Error(`${relativePath} fails ${schemaId}:\n${details}`);
  }
  return value;
}

async function listJson(directory) {
  try {
    return (await readdir(directory)).filter((fileName) => fileName.endsWith(".json")).sort();
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }
}

function run(command, argumentsList, capture = false) {
  const result = spawnSync(command, argumentsList, {
    cwd: repoRoot,
    encoding: capture ? undefined : "utf8",
    stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
  return capture ? result.stdout : undefined;
}
