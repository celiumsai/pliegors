// SPDX-License-Identifier: Apache-2.0

import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { lstat, mkdir, readdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

export const NEXT_AUTHORITY = Object.freeze({
  releasedTag: "v0.4.0-beta.1",
  releasedRevision: "9cdadea508dfbf78b2ec5061df6846e7fa727211",
  preNextRevision: "bdc285fbe208f09c47ffdfbf1081e923884a9cf7",
});

export const NEXT_FIXTURE_IDS = Object.freeze([
  "minimal",
  "stress-dashboard",
  "hyphae-console",
]);

export const NEXT_CASE_IDS = Object.freeze([
  "build-cold",
  "build-warm-verified",
  "phase-discovery",
  "phase-rust-wasm",
  "phase-site",
  "phase-transform",
  "phase-verification",
  "dev-start-cold",
  "dev-start-warm",
  "change-to-server",
  "change-to-browser-visible",
  "hmr-phase-discovery",
  "hmr-phase-rust-wasm",
  "hmr-phase-site",
  "hmr-phase-verification",
  "hmr-host-transport-overhead",
  "dom-mutations",
  "peak-rss",
  "long-session-rss",
  "active-owners",
  "active-listeners",
  "active-effects",
  "mounted-roots",
  "wasm-bytes",
  "asset-bytes",
  "cache-attempts",
  "cache-hits",
  "cache-misses",
  "cache-reused-artifacts",
  "cache-hit-rate",
  "remount-count",
  "full-reload-count",
]);

const FIXTURE_SCHEMA_ID = "https://pliegors.dev/schemas/pliego.next-fixture-manifest.schema.json";
const BASELINE_SCHEMA_ID = "https://pliegors.dev/schemas/pliego.next-baseline-report.schema.json";
const MAX_FIXTURE_FILES = 4096;
const MAX_FIXTURE_BYTES = 64 * 1024 * 1024;
const GENERATED_FIXTURE_ROOTS = new Set(["target"]);

export async function loadNextBaselineContracts(root) {
  const [fixtureSchema, baselineSchema] = await Promise.all([
    readJson(path.join(root, "schemas", "pliego.next-fixture-manifest.schema.json")),
    readJson(path.join(root, "schemas", "pliego.next-baseline-report.schema.json")),
  ]);
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  addFormats(ajv);
  ajv.addSchema(fixtureSchema);
  ajv.addSchema(baselineSchema);
  return {
    validateFixture: ajv.getSchema(FIXTURE_SCHEMA_ID),
    validateBaseline: ajv.getSchema(BASELINE_SCHEMA_ID),
    errorsText: (errors) => ajv.errorsText(errors, { separator: "\n" }),
  };
}

export async function validateFixtureManifest(manifest, contracts, root) {
  if (!contracts.validateFixture(manifest)) {
    throw new Error(`fixture manifest schema: ${contracts.errorsText(contracts.validateFixture.errors)}`);
  }
  assertDeepEqual(manifest.authority, NEXT_AUTHORITY, "fixture authority differs from ADR-007");
  assertDeepEqual(manifest.fixtures.map((fixture) => fixture.id), NEXT_FIXTURE_IDS, "fixture IDs or order differ");
  assertDeepEqual(manifest.measurementCases.map((item) => item.id), NEXT_CASE_IDS, "measurement cases or order differ");
  assertUnique(manifest.measurementCases.map((item) => item.id), "measurement case ID");
  assertUnique(manifest.fixtures.map((fixture) => fixture.id), "fixture ID");

  const cases = new Map(manifest.measurementCases.map((item) => [item.id, item]));
  for (const candidate of manifest.policy.bottlenecks.candidateCaseIds) {
    const measurementCase = cases.get(candidate);
    assert(measurementCase, `unknown bottleneck case: ${candidate}`);
    assert(measurementCase.unit === "milliseconds", `bottleneck case is not a duration: ${candidate}`);
  }

  for (const fixture of manifest.fixtures) {
    const fixtureRoot = resolveInside(root, fixture.root);
    const descriptorPath = resolveInside(root, fixture.descriptor);
    assert(isInside(fixtureRoot, descriptorPath), `${fixture.id} descriptor is outside its fixture root`);
    const descriptor = await readJson(descriptorPath);
    assert(descriptor.contract === "dev.pliegors.next-fixture/v1", `${fixture.id} descriptor contract differs`);
    assert(descriptor.id === fixture.id, `${fixture.id} descriptor identity differs`);
    assert(descriptor.workload === fixture.workload, `${fixture.id} workload differs from its descriptor`);
    const identity = await fixtureTreeIdentity(fixtureRoot);
    assertDeepEqual(identity, fixture.sourceIdentity, `${fixture.id} source identity differs`);
    assertUnique(fixture.notApplicable.map((item) => item.caseId), `${fixture.id} not-applicable case`);
    for (const declaration of fixture.notApplicable) {
      assert(cases.has(declaration.caseId), `${fixture.id} declares unknown not-applicable case ${declaration.caseId}`);
    }
  }
}

export async function validateBaselineReport(report, manifest, contracts, root) {
  if (!contracts.validateBaseline(report)) {
    throw new Error(`baseline report schema: ${contracts.errorsText(contracts.validateBaseline.errors)}`);
  }
  assert(Date.parse(report.completedAt) >= Date.parse(report.createdAt), "baseline completedAt precedes createdAt");
  assertDeepEqual(report.authority, manifest.authority, "baseline authority differs from fixture manifest");
  assertDeepEqual(report.fixtureManifestIdentity, fixtureManifestIdentity(manifest), "baseline manifest identity differs");
  assertDeepEqual(report.fixtures.map((fixture) => fixture.id), NEXT_FIXTURE_IDS, "baseline fixture IDs or order differ");
  assert(report.policy.warmupRuns === manifest.policy.warmupRuns, "baseline warmup policy differs");
  assert(report.policy.measuredRuns === manifest.policy.measuredRuns, "baseline sample policy differs");
  assert(report.policy.percentileMethod === manifest.policy.percentileMethod, "baseline percentile policy differs");
  assert(report.policy.outlierPolicy === manifest.policy.outlierPolicy, "baseline outlier policy differs");

  const cases = new Map(manifest.measurementCases.map((item) => [item.id, item]));
  for (const [index, fixtureResult] of report.fixtures.entries()) {
    const fixture = manifest.fixtures[index];
    assertDeepEqual(fixtureResult.sourceIdentity, fixture.sourceIdentity, `${fixture.id} baseline source identity differs`);
    assertDeepEqual(fixtureResult.results.map((result) => result.caseId), NEXT_CASE_IDS, `${fixture.id} result matrix differs`);
    const notApplicable = new Map(fixture.notApplicable.map((item) => [item.caseId, item]));
    for (const result of fixtureResult.results) {
      const measurementCase = cases.get(result.caseId);
      assert(result.unit === measurementCase.unit, `${fixture.id}/${result.caseId} unit differs`);
      if (result.status === "measured") validateMeasuredResult(result, measurementCase, manifest.policy.measuredRuns);
      if (result.status === "not-applicable") {
        const declaration = notApplicable.get(result.caseId);
        assert(declaration, `${fixture.id}/${result.caseId} is undeclared not-applicable`);
        assert(result.reasonCode === declaration.reasonCode, `${fixture.id}/${result.caseId} not-applicable reason differs`);
      }
    }
    validateCacheResults(fixtureResult.results, manifest.policy.measuredRuns);
  }

  if (report.status === "complete") {
    assert(!report.sourceTreeDirty, "complete baseline cannot come from a dirty source tree");
    const unavailable = report.fixtures.flatMap((fixture) => fixture.results).filter((result) => result.status === "unavailable");
    assert(unavailable.length === 0, "complete baseline contains unavailable measurements");
    const bottlenecks = selectBottlenecks(report, manifest);
    assert(bottlenecks.length === manifest.policy.bottlenecks.count, "complete baseline lacks three measured bottlenecks");
    assertDeepEqual(report.bottlenecks, bottlenecks, "complete baseline bottlenecks differ");
  } else {
    assert(report.bottlenecks.length === 0, "incomplete baseline cannot claim measured bottlenecks");
  }

  for (const fixture of manifest.fixtures) {
    const identity = await fixtureTreeIdentity(resolveInside(root, fixture.root));
    assertDeepEqual(identity, fixture.sourceIdentity, `${fixture.id} source changed after baseline construction`);
  }
}

export function nearestRankSummary(values) {
  assert(Array.isArray(values) && values.length > 0, "cannot summarize an empty observation set");
  assert(values.every((value) => Number.isFinite(value) && value >= 0), "observations must be finite non-negative numbers");
  const ordered = [...values].sort((left, right) => left - right);
  return {
    p50: nearestRank(ordered, 0.50),
    p95: nearestRank(ordered, 0.95),
    p99: nearestRank(ordered, 0.99),
  };
}

export async function fixtureTreeIdentity(root) {
  const rootDetails = await lstat(root);
  assert(rootDetails.isDirectory() && !rootDetails.isSymbolicLink(), `fixture root is not a regular directory: ${root}`);
  const files = [];
  await collectFixtureFiles(root, root, files);
  files.sort((left, right) => left.relativePath.localeCompare(right.relativePath, "en"));
  assert(files.length > 0, `fixture has no files: ${root}`);
  assert(files.length <= MAX_FIXTURE_FILES, `fixture exceeds ${MAX_FIXTURE_FILES} files: ${root}`);
  const folded = new Set();
  let totalBytes = 0;
  const hash = createHash("sha256");
  hash.update("dev.pliegors.fixture-tree/v1\0");
  for (const file of files) {
    const collisionKey = file.relativePath.normalize("NFKC").toLocaleLowerCase("en-US");
    assert(!folded.has(collisionKey), `fixture contains a case or Unicode path collision: ${file.relativePath}`);
    folded.add(collisionKey);
    const bytes = await readFile(file.absolutePath);
    totalBytes += bytes.length;
    assert(totalBytes <= MAX_FIXTURE_BYTES, `fixture exceeds ${MAX_FIXTURE_BYTES} bytes: ${root}`);
    hash.update(`${Buffer.byteLength(file.relativePath)}:`);
    hash.update(file.relativePath);
    hash.update(`:${bytes.length}:`);
    hash.update(bytes);
    hash.update("\0");
  }
  return { algorithm: "sha256-tree-v1", digest: hash.digest("hex") };
}

export function fixtureManifestIdentity(manifest) {
  return {
    algorithm: "sha256-canonical-json-v1",
    digest: createHash("sha256").update(canonicalJson(manifest)).digest("hex"),
  };
}

export async function buildInventoryReport({ manifest, contracts, root, now = new Date().toISOString() }) {
  await validateFixtureManifest(manifest, contracts, root);
  const cases = new Map(manifest.measurementCases.map((item) => [item.id, item]));
  const stages = new Map(await Promise.all(manifest.fixtures.map(async (fixture) => [
    fixture.id,
    (await readJson(path.resolve(root, ...fixture.descriptor.split("/")))).stage,
  ])));
  const revision = commandText("git", ["rev-parse", "--verify", "HEAD"], root);
  const dirty = commandText("git", ["status", "--porcelain", "--untracked-files=normal"], root) !== "";
  return {
    $schema: BASELINE_SCHEMA_ID,
    contract: "dev.pliegors.next-baseline-report/v1",
    status: "incomplete",
    createdAt: now,
    completedAt: now,
    sourceRevision: revision,
    sourceTreeDirty: dirty,
    fixtureManifestPath: "fixtures/next/manifest.json",
    fixtureManifestIdentity: fixtureManifestIdentity(manifest),
    authority: structuredClone(manifest.authority),
    policy: {
      warmupRuns: manifest.policy.warmupRuns,
      measuredRuns: manifest.policy.measuredRuns,
      percentileMethod: manifest.policy.percentileMethod,
      outlierPolicy: manifest.policy.outlierPolicy,
    },
    environment: environmentReport(root),
    fixtures: manifest.fixtures.map((fixture) => ({
      id: fixture.id,
      sourceIdentity: structuredClone(fixture.sourceIdentity),
      results: NEXT_CASE_IDS.map((caseId) => {
        const declared = fixture.notApplicable.find((item) => item.caseId === caseId);
        if (declared) {
          return {
            caseId,
            unit: cases.get(caseId).unit,
            status: "not-applicable",
            reasonCode: declared.reasonCode,
            explanation: declared.explanation,
          };
        }
        const deferred = stages.get(fixture.id) === "deferred";
        return {
          caseId,
          unit: cases.get(caseId).unit,
          status: "unavailable",
          reasonCode: deferred ? "FIXTURE_DEFERRED" : "COLLECTOR_NOT_EXECUTED",
          explanation: deferred
            ? "The fixture is deliberately deferred behind an external product and integration gate."
            : "The fixture is executable, but this collector was not selected for the current report.",
        };
      }),
    })),
    bottlenecks: [],
    limitations: [
      "This inventory report freezes the fixture and metric contract; it is not accepted performance evidence.",
      "Unavailable instrumentation is reported explicitly and is never substituted with a zero measurement.",
      "The first accepted baseline requires executable fixtures, five independent samples, and exactly three measured bottlenecks.",
    ],
  };
}

export function renderBaselineMarkdown(report, manifest) {
  const cases = new Map(manifest.measurementCases.map((item) => [item.id, item]));
  const lines = [
    "# PliegoRS Next baseline",
    "",
    `**Status:** ${report.status.toUpperCase()}`,
    `**Source revision:** \`${report.sourceRevision}\``,
    `**Source tree dirty:** ${report.sourceTreeDirty ? "yes" : "no"}`,
    `**Captured:** ${report.completedAt}`,
    "",
    "## Authority",
    "",
    `- Released reference: \`${report.authority.releasedTag}\` at \`${report.authority.releasedRevision}\``,
    `- Pre-Next reference: \`${report.authority.preNextRevision}\``,
    `- Fixture manifest: \`${report.fixtureManifestIdentity.algorithm}:${report.fixtureManifestIdentity.digest}\``,
    "",
    "## Environment",
    "",
    "| Field | Value |",
    "| --- | --- |",
    `| Platform | ${cell(`${report.environment.platform} ${report.environment.architecture} ${report.environment.osRelease}`)} |`,
    `| CPU | ${cell(report.environment.cpuModel)} |`,
    `| Logical CPUs | ${report.environment.logicalCpuCount} |`,
    `| Memory bytes | ${report.environment.totalMemoryBytes} |`,
    `| Storage | ${cell(report.environment.storageClass)} |`,
    `| Node | ${cell(report.environment.node)} |`,
    `| Rust | ${cell(firstLine(report.environment.rustc))} |`,
    `| Cargo | ${cell(firstLine(report.environment.cargo))} |`,
    `| Browser | ${cell(report.environment.browser)} |`,
    "",
    "## Measurements",
    "",
    "| Fixture | Case | Unit | Status | p50 | p95 | p99 | Reason |",
    "| --- | --- | --- | --- | ---: | ---: | ---: | --- |",
  ];
  for (const fixture of report.fixtures) {
    for (const result of fixture.results) {
      const label = cases.get(result.caseId)?.description ?? result.caseId;
      const summary = result.status === "measured" ? result.summary : null;
      const reason = result.status === "measured" ? "" : `${result.reasonCode}: ${result.explanation}`;
      lines.push(`| ${fixture.id} | ${cell(label)} | ${result.unit} | ${result.status} | ${summary?.p50 ?? "-"} | ${summary?.p95 ?? "-"} | ${summary?.p99 ?? "-"} | ${cell(reason)} |`);
    }
  }
  lines.push("", "## Bottlenecks", "");
  if (report.bottlenecks.length === 0) {
    lines.push("No bottlenecks are claimed until the baseline is complete.");
  } else {
    for (const item of report.bottlenecks) {
      lines.push(`${item.rank}. \`${item.fixtureId}/${item.caseId}\`: ${item.value} ${item.unit} (${item.statistic})`);
    }
  }
  lines.push("", "## Limitations", "");
  for (const limitation of report.limitations) lines.push(`- ${limitation}`);
  return `${lines.join("\n")}\n`;
}

export async function publishBaselineDirectory(output, report, markdown) {
  const parent = path.dirname(output);
  await mkdir(parent, { recursive: true });
  if (await pathExists(output)) throw new Error(`baseline output already exists: ${output}`);
  const stage = path.join(parent, `.${path.basename(output)}.stage-${process.pid}-${randomUUID()}`);
  await mkdir(stage, { recursive: false });
  try {
    const json = `${JSON.stringify(report, null, 2)}\n`;
    await Promise.all([
      writeFile(path.join(stage, "baseline.json"), json, { encoding: "utf8", flag: "wx" }),
      writeFile(path.join(stage, "baseline.md"), markdown, { encoding: "utf8", flag: "wx" }),
    ]);
    assert(await readFile(path.join(stage, "baseline.json"), "utf8") === json, "staged baseline JSON changed");
    assert(await readFile(path.join(stage, "baseline.md"), "utf8") === markdown, "staged baseline Markdown changed");
    if (await pathExists(output)) throw new Error(`baseline output already exists: ${output}`);
    await rename(stage, output);
  } catch (error) {
    await rm(stage, { recursive: true, force: true });
    throw error;
  }
}

function validateMeasuredResult(result, measurementCase, measuredRuns) {
  assert(result.observations.length === measuredRuns, `${result.caseId} observation count differs`);
  for (const [index, observation] of result.observations.entries()) {
    assert(observation.sample === index + 1, `${result.caseId} sample sequence differs`);
    if (["bytes", "count"].includes(result.unit)) {
      assert(Number.isSafeInteger(observation.value), `${result.caseId} requires safe integer observations`);
    }
    if (result.unit === "ratio") assert(observation.value <= 1, `${result.caseId} ratio exceeds one`);
    if (measurementCase.category === "hmr") {
      assert(Array.isArray(observation.reasons), `${result.caseId} requires typed reason counts`);
      assertUnique(observation.reasons.map((reason) => reason.code), `${result.caseId} reason code`);
      assert(observation.reasons.reduce((sum, reason) => sum + reason.count, 0) === observation.value, `${result.caseId} reason counts do not equal the observation`);
    } else {
      assert(!observation.reasons, `${result.caseId} reasons are allowed only for HMR cases`);
    }
  }
  const expected = nearestRankSummary(result.observations.map((observation) => observation.value));
  assertDeepEqual(result.summary, expected, `${result.caseId} summary differs from raw observations`);
}

function validateCacheResults(results, measuredRuns) {
  const byId = new Map(results.map((result) => [result.caseId, result]));
  const cacheIds = ["cache-attempts", "cache-hits", "cache-misses", "cache-hit-rate"];
  const measured = cacheIds.map((id) => byId.get(id)).filter((result) => result.status === "measured");
  if (measured.length === 0) return;
  assert(measured.length === cacheIds.length, "cache metrics must become measured together");
  for (let index = 0; index < measuredRuns; index += 1) {
    const attempts = byId.get("cache-attempts").observations[index].value;
    const hits = byId.get("cache-hits").observations[index].value;
    const misses = byId.get("cache-misses").observations[index].value;
    const rate = byId.get("cache-hit-rate").observations[index].value;
    assert(attempts === hits + misses, `cache sample ${index + 1} attempts differ from hits plus misses`);
    const expectedRate = attempts === 0 ? 0 : hits / attempts;
    assert(Math.abs(rate - expectedRate) <= Number.EPSILON * 4, `cache sample ${index + 1} hit rate differs`);
  }
}

function selectBottlenecks(report, manifest) {
  const candidates = new Set(manifest.policy.bottlenecks.candidateCaseIds);
  return report.fixtures
    .flatMap((fixture) => fixture.results
      .filter((result) => result.status === "measured" && candidates.has(result.caseId))
      .map((result) => ({
        fixtureId: fixture.id,
        caseId: result.caseId,
        value: result.summary.p95,
      })))
    .sort((left, right) => right.value - left.value
      || left.fixtureId.localeCompare(right.fixtureId, "en")
      || left.caseId.localeCompare(right.caseId, "en"))
    .slice(0, manifest.policy.bottlenecks.count)
    .map((item, index) => ({
      rank: index + 1,
      fixtureId: item.fixtureId,
      caseId: item.caseId,
      statistic: "p95",
      value: item.value,
      unit: "milliseconds",
    }));
}

async function collectFixtureFiles(root, current, files) {
  const entries = await readdir(current, { withFileTypes: true });
  entries.sort((left, right) => left.name.localeCompare(right.name, "en"));
  for (const entry of entries) {
    if (current === root && GENERATED_FIXTURE_ROOTS.has(entry.name)) continue;
    const absolutePath = path.join(current, entry.name);
    const details = await lstat(absolutePath);
    const relativePath = path.relative(root, absolutePath).split(path.sep).join("/");
    assert(!details.isSymbolicLink(), `fixture contains a symbolic link: ${relativePath}`);
    if (details.isDirectory()) await collectFixtureFiles(root, absolutePath, files);
    else if (details.isFile()) files.push({ absolutePath, relativePath });
    else throw new Error(`fixture contains a non-regular entry: ${relativePath}`);
  }
}

function environmentReport(root) {
  return {
    platform: process.platform,
    architecture: process.arch,
    osRelease: os.release(),
    cpuModel: os.cpus()[0]?.model?.trim() ?? "unknown",
    logicalCpuCount: os.cpus().length,
    totalMemoryBytes: os.totalmem(),
    storageClass: process.env.PLIEGO_NEXT_STORAGE_CLASS ?? "not declared",
    powerAndThermalState: "not controlled or measured",
    node: process.version,
    rustc: commandText("rustc", ["--version", "--verbose"], root),
    cargo: commandText("cargo", ["--version", "--verbose"], root),
    browser: "not collected",
  };
}

function nearestRank(ordered, percentile) {
  return ordered[Math.max(0, Math.ceil(ordered.length * percentile) - 1)];
}

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function resolveInside(root, relativePath) {
  assert(path.posix.normalize(relativePath) === relativePath, `path is not canonical: ${relativePath}`);
  const resolved = path.resolve(root, ...relativePath.split("/"));
  assert(isInside(root, resolved), `path escapes repository: ${relativePath}`);
  return resolved;
}

function isInside(parent, child) {
  const relative = path.relative(parent, child);
  return relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative);
}

function commandText(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    windowsHide: true,
    maxBuffer: 4 * 1024 * 1024,
  });
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed: ${(result.stderr ?? "").trim()}`);
  return (result.stdout ?? "").trim();
}

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

async function pathExists(file) {
  try {
    await lstat(file);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function firstLine(value) {
  return value.split(/\r?\n/u, 1)[0];
}

function cell(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\r", " ").replaceAll("\n", " ");
}

function assertUnique(values, label) {
  assert(new Set(values).size === values.length, `duplicate ${label}`);
}

function assertDeepEqual(actual, expected, message) {
  assert(JSON.stringify(actual) === JSON.stringify(expected), message);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
