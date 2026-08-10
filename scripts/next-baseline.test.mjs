import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  buildInventoryReport,
  fixtureTreeIdentity,
  loadNextBaselineContracts,
  nearestRankSummary,
  publishBaselineDirectory,
  renderBaselineMarkdown,
  validateBaselineReport,
  validateFixtureManifest,
} from "./next-baseline-lib.mjs";
import { checkNextBaseline } from "./check-next-baseline.mjs";
import {
  applyFixtureBuildMeasurements,
  parseBuildLedger,
  parseBuildRecord,
  projectFixtureBuildMetrics,
} from "./next-baseline-build.mjs";
import {
  applyFixtureDevMeasurements,
  parseDevUpdate,
  parseRebuildRecord,
  parseSseFrame,
  projectFixtureDevMetrics,
} from "./next-baseline-dev.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

test("canonical Next fixture registry has exact authority and source identities", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));

  await validateFixtureManifest(manifest, contracts, root);

  assert.deepEqual(manifest.fixtures.map((fixture) => fixture.id), [
    "minimal",
    "stress-dashboard",
    "hyphae-console",
  ]);
});

test("fixture validation rejects registry, source, and external authority drift", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const extra = structuredClone(manifest);
  extra.fixtures.push(structuredClone(extra.fixtures[0]));
  extra.fixtures[3].id = "surprise";
  await assert.rejects(validateFixtureManifest(extra, contracts, root), /fixture manifest schema/u);

  const changed = structuredClone(manifest);
  changed.fixtures[0].sourceIdentity.digest = "0".repeat(64);
  await assert.rejects(validateFixtureManifest(changed, contracts, root), /source identity/u);

  const changedAuthority = structuredClone(manifest);
  changedAuthority.fixtures[2].externalAuthority.releaseTag = "v1.0.2";
  await assert.rejects(validateFixtureManifest(changedAuthority, contracts, root), /Hyphae authority/u);

  const missingAuthority = structuredClone(manifest);
  delete missingAuthority.fixtures[2].externalAuthority;
  await assert.rejects(validateFixtureManifest(missingAuthority, contracts, root), /fixture manifest schema/u);

  const misplacedAuthority = structuredClone(manifest);
  misplacedAuthority.fixtures[0].externalAuthority = structuredClone(manifest.fixtures[2].externalAuthority);
  await assert.rejects(validateFixtureManifest(misplacedAuthority, contracts, root), /fixture manifest schema/u);
});

test("nearest-rank summary retains p99 as the maximum for five samples", () => {
  assert.deepEqual(nearestRankSummary([4, 1, 5, 2, 3]), {
    p50: 3,
    p95: 5,
    p99: 5,
  });
});

test("inventory report distinguishes specified fixtures without inventing measurements", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const report = await buildInventoryReport({ manifest, contracts, root, now: "2026-08-08T12:00:00.000Z" });

  await validateBaselineReport(report, manifest, contracts, root);

  assert.equal(report.status, "incomplete");
  assert.equal(report.bottlenecks.length, 0);
  assert.ok(report.fixtures.every((fixture) => fixture.results.every((result) => result.status === "unavailable")));
  assert.ok(report.fixtures.every((fixture) => fixture.results.every((result) => !("observations" in result))));
  assert.equal(report.fixtures[0].results[0].reasonCode, "COLLECTOR_NOT_EXECUTED");
  assert.equal(report.fixtures[2].results[0].reasonCode, "FIXTURE_NOT_EXECUTABLE");
  assert.match(report.fixtures[2].results[0].explanation, /specified but not executable/u);
});

test("baseline validation rejects unavailable results marked complete", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const report = await buildInventoryReport({ manifest, contracts, root, now: "2026-08-08T12:00:00.000Z" });
  report.status = "complete";
  report.sourceTreeDirty = false;

  await assert.rejects(validateBaselineReport(report, manifest, contracts, root), /complete baseline contains unavailable/u);
});

test("baseline validation recalculates measured percentiles", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const report = await buildInventoryReport({ manifest, contracts, root, now: "2026-08-08T12:00:00.000Z" });
  const result = report.fixtures[0].results[0];
  Object.assign(result, {
    status: "measured",
    observations: [1, 2, 3, 4, 5].map((value, index) => ({ sample: index + 1, value })),
    summary: { p50: 99, p95: 5, p99: 5 },
  });
  delete result.reasonCode;
  delete result.explanation;

  await assert.rejects(validateBaselineReport(report, manifest, contracts, root), /summary differs/u);
});

test("Markdown is a deterministic rendering of baseline JSON", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const report = await buildInventoryReport({ manifest, contracts, root, now: "2026-08-08T12:00:00.000Z" });

  const first = renderBaselineMarkdown(report, manifest);
  const second = renderBaselineMarkdown(structuredClone(report), structuredClone(manifest));

  assert.equal(first, second);
  assert.match(first, /Status:\*\* INCOMPLETE/u);
  assert.ok(first.endsWith("\n"));
});

test("baseline publication is exact-set and refuses an existing destination", async (t) => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), "pliego-next-baseline-test-"));
  t.after(() => rm(temporary, { recursive: true, force: true }));
  const output = path.join(temporary, "baseline");
  const report = { contract: "test" };
  const markdown = "# test\n";

  await publishBaselineDirectory(output, report, markdown);
  assert.deepEqual(JSON.parse(await readFile(path.join(output, "baseline.json"), "utf8")), report);
  assert.equal(await readFile(path.join(output, "baseline.md"), "utf8"), markdown);
  await assert.rejects(publishBaselineDirectory(output, report, markdown), /already exists/u);
});

test("baseline checker rejects extra output files", async (t) => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), "pliego-next-baseline-check-"));
  const output = path.join(temporary, "baseline");
  t.after(() => rm(temporary, { recursive: true, force: true }));
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const report = await buildInventoryReport({ manifest, contracts, root, now: "2026-08-08T12:00:00.000Z" });
  await publishBaselineDirectory(output, report, renderBaselineMarkdown(report, manifest));
  await writeFile(path.join(output, "extra.txt"), "unexpected\n");

  await assert.rejects(
    checkNextBaseline({ repositoryRoot: root, baselineDirectory: output }),
    /exactly baseline\.json and baseline\.md/u,
  );
});

test("fixture identity rejects symbolic links when the platform permits them", async (t) => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), "pliego-next-fixture-test-"));
  t.after(() => rm(temporary, { recursive: true, force: true }));
  await writeFile(path.join(temporary, "source.txt"), "source\n");
  try {
    const { symlink } = await import("node:fs/promises");
    await symlink(path.join(temporary, "source.txt"), path.join(temporary, "alias.txt"));
  } catch (error) {
    if (["EPERM", "EACCES", "ENOSYS"].includes(error?.code)) return;
    throw error;
  }
  await assert.rejects(fixtureTreeIdentity(temporary), /symbolic link/u);
});

test("fixture identity excludes only the generated root target directory", async (t) => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), "pliego-next-fixture-target-"));
  t.after(() => rm(temporary, { recursive: true, force: true }));
  await writeFile(path.join(temporary, "source.txt"), "source\n");
  const before = await fixtureTreeIdentity(temporary);
  const { mkdir } = await import("node:fs/promises");
  await mkdir(path.join(temporary, "target"));
  await writeFile(path.join(temporary, "target", "generated.bin"), "generated\n");

  assert.deepEqual(await fixtureTreeIdentity(temporary), before);
});

test("minimal build collector validates cold and warm records without inventing cache hits", () => {
  const cold = parseBuildRecord(buildRecord({
    outcome: "executed",
    renderedArtifacts: 5,
    reusedArtifacts: 0,
    receiptBefore: null,
    phases: {
      discoveryMicros: 1_000,
      clientMicros: 2_000,
      siteMicros: 3_000,
      verificationMicros: 4_000,
      totalMicros: 10_000,
    },
  }));
  const warm = parseBuildRecord(buildRecord({
    outcome: "no-op",
    renderedArtifacts: 0,
    reusedArtifacts: 5,
    receiptBefore: "a".repeat(64),
    phases: {
      discoveryMicros: 1_000,
      clientMicros: 0,
      siteMicros: 0,
      verificationMicros: 0,
      totalMicros: 1_000,
    },
  }));

  assert.equal(cold.outcome, "executed");
  assert.equal(warm.outcome, "no-op");
  assert.ok(!Object.hasOwn(warm, "cacheHits"));
  assert.throws(
    () => parseBuildRecord({ ...buildRecord(), cacheHits: 5 }),
    /unexpected fields/u,
  );
});

test("minimal ledger collector reconciles output bytes and separates WASM from assets", () => {
  const ledger = parseBuildLedger(buildLedger());

  assert.equal(ledger.wasmBytes, 20);
  assert.equal(ledger.assetBytes, 30);
  assert.equal(ledger.totalBytes, 45);

  const changed = buildLedger();
  changed.receipt.outputs.totalBytes = 44;
  assert.throws(() => parseBuildLedger(changed), /totalBytes differs/u);
});

test("minimal metric projection measures only supported build evidence", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const report = await buildInventoryReport({ manifest, contracts, root, now: "2026-08-08T12:00:00.000Z" });
  const samples = Array.from({ length: 5 }, (_, index) => minimalRawSample(index + 1));
  const metrics = projectFixtureBuildMetrics(samples, manifest);
  const projected = applyFixtureBuildMeasurements(report, [{ fixtureId: "minimal", metrics, peakRssMeasured: true }]);
  const minimal = projected.fixtures.find((fixture) => fixture.id === "minimal");

  assert.equal(minimal.results.find((result) => result.caseId === "build-cold").status, "measured");
  assert.equal(minimal.results.find((result) => result.caseId === "wasm-bytes").summary.p50, 20);
  assert.equal(minimal.results.find((result) => result.caseId === "asset-bytes").summary.p50, 30);
  assert.equal(minimal.results.find((result) => result.caseId === "cache-reused-artifacts").summary.p50, 5);
  for (const caseId of ["cache-attempts", "cache-hits", "cache-misses", "cache-hit-rate"]) {
    const result = minimal.results.find((candidate) => candidate.caseId === caseId);
    assert.equal(result.status, "unavailable");
    assert.equal(result.reasonCode, "COLLECTOR_NOT_IMPLEMENTED");
  }
  assert.equal(projected.status, "incomplete");
  assert.deepEqual(projected.bottlenecks, []);
});

test("build metric application supports minimal and stress dashboard in one batch", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const report = await buildInventoryReport({ manifest, contracts, root, now: "2026-08-08T12:00:00.000Z" });
  const metrics = projectFixtureBuildMetrics(
    Array.from({ length: 5 }, (_, index) => minimalRawSample(index + 1)),
    manifest,
  );
  const projected = applyFixtureBuildMeasurements(report, [
    { fixtureId: "minimal", metrics, peakRssMeasured: true },
    { fixtureId: "stress-dashboard", metrics, peakRssMeasured: true },
  ]);

  assert.equal(projected.fixtures[0].results[0].status, "measured");
  assert.equal(projected.fixtures[1].results[0].status, "measured");
  assert.equal(projected.fixtures[2].results[0].status, "unavailable");
  assert.match(projected.limitations[0], /minimal, stress-dashboard/u);
  assert.match(projected.limitations[1], /hyphae-console/u);
  assert.throws(
    () => applyFixtureBuildMeasurements(report, [
      { fixtureId: "minimal", metrics, peakRssMeasured: true },
      { fixtureId: "minimal", metrics, peakRssMeasured: true },
    ]),
    /duplicate executed fixture ID/u,
  );
});

test("dev SSE parser accepts heartbeat and strict CSS updates", () => {
  assert.deepEqual(parseSseFrame(": heartbeat\n\n", 0), { type: "heartbeat" });
  const update = parseSseFrame(
    'event: pliego\ndata: {"generation":2,"kind":"css","paths":["/assets/style.css"],"routes":["/"]}\n\n',
    0,
  );
  assert.equal(update.type, "update");
  assert.deepEqual(update.update, {
    generation: 2,
    kind: "css",
    paths: ["/assets/style.css"],
    routes: ["/"],
  });
  assert.throws(
    () => parseSseFrame('event: pliego\ndata: {"generation":1,"kind":"css","paths":["../style.css"],"routes":[]}\n\n', 0),
    /path is invalid/u,
  );
});

test("rebuild record must match the observed dev update", () => {
  const update = parseDevUpdate({
    generation: 1,
    kind: "css",
    paths: ["/assets/style.css"],
    routes: ["/"],
  }, 0);
  const record = parseRebuildRecord({
    recordVersion: "pliego-rebuild/1",
    generation: 1,
    changedSources: ["site/style.css"],
    affectedRoutes: ["/"],
    affectedArtifacts: ["assets/style.css", "index.html"],
    changedArtifacts: ["assets/style.css"],
    hmr: { kind: "css", paths: ["/assets/style.css"], routes: ["/"] },
    receiptBefore: "a".repeat(64),
    receiptAfter: "b".repeat(64),
  }, update);
  assert.equal(record.generation, 1);
  assert.throws(
    () => parseRebuildRecord({ ...record, generation: 2 }, update),
    /generation differs/u,
  );
});

test("dev measurements preserve build evidence and keep typed reload counts unavailable", async () => {
  const contracts = await loadNextBaselineContracts(root);
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const inventory = await buildInventoryReport({ manifest, contracts, root, now: "2026-08-08T12:00:00.000Z" });
  const buildMetrics = projectFixtureBuildMetrics(
    Array.from({ length: 5 }, (_, index) => minimalRawSample(index + 1)),
    manifest,
  );
  const withBuild = applyFixtureBuildMeasurements(inventory, [
    { fixtureId: "minimal", metrics: buildMetrics, peakRssMeasured: true },
  ]);
  const devMetrics = projectFixtureDevMetrics(
    Array.from({ length: 5 }, (_, index) => ({
      sample: index + 1,
      coldStartMs: 100 + index,
      warmStartMs: 20 + index,
      serverUpdateMs: 10 + index,
      browserVisibleMs: 12 + index,
      hmrDiscoveryMs: 2 + index,
      hmrRustWasmMs: 3 + index,
      hmrSiteMs: 4 + index,
      hmrVerificationMs: 1 + index,
      hmrHostTransportMs: 5 + index,
      domMutations: 1,
      longSessionRssBytes: 1_000 + index,
    })),
    manifest,
  );
  const projected = applyFixtureDevMeasurements(withBuild, [
    { fixtureId: "minimal", metrics: devMetrics, browser: "Chrome/test" },
  ]);
  const minimal = projected.fixtures[0];

  assert.equal(minimal.results.find((result) => result.caseId === "build-cold").status, "measured");
  assert.equal(minimal.results.find((result) => result.caseId === "dev-start-cold").status, "measured");
  assert.equal(minimal.results.find((result) => result.caseId === "dom-mutations").summary.p50, 1);
  assert.equal(minimal.results.find((result) => result.caseId === "hmr-phase-site").status, "measured");
  assert.equal(minimal.results.find((result) => result.caseId === "long-session-rss").status, "measured");
  for (const caseId of ["remount-count", "full-reload-count"]) {
    const result = minimal.results.find((candidate) => candidate.caseId === caseId);
    assert.equal(result.status, "unavailable");
    assert.equal(result.reasonCode, "TYPED_HMR_RECEIPT_UNAVAILABLE");
  }
  assert.equal(projected.environment.browser, "Chrome/test");
});

test("dev projection rejects negative residual host transport time", async () => {
  const manifest = JSON.parse(await readFile(path.join(root, "fixtures", "next", "manifest.json"), "utf8"));
  const samples = Array.from({ length: 5 }, (_, index) => ({
    sample: index + 1,
    coldStartMs: 100,
    warmStartMs: 20,
    serverUpdateMs: 10,
    browserVisibleMs: 12,
    hmrDiscoveryMs: 2,
    hmrRustWasmMs: 3,
    hmrSiteMs: 4,
    hmrVerificationMs: 1,
    hmrHostTransportMs: index === 2 ? -1 : 0,
    domMutations: 1,
    longSessionRssBytes: 1_000,
  }));

  assert.throws(() => projectFixtureDevMetrics(samples, manifest), /hmrHostTransportMs is invalid/u);
});

function buildRecord(overrides = {}) {
  return {
    recordVersion: "pliego-build/1",
    outcome: "executed",
    changedSources: [],
    globalInvalidation: false,
    renderedArtifacts: 5,
    reusedArtifacts: 0,
    receiptBefore: null,
    receiptAfter: "a".repeat(64),
    phases: {
      discoveryMicros: 1_000,
      clientMicros: 2_000,
      siteMicros: 3_000,
      verificationMicros: 4_000,
      totalMicros: 10_000,
    },
    ...overrides,
  };
}

function buildLedger() {
  return {
    reportVersion: "2.0.0",
    receiptSha256: "a".repeat(64),
    receipt: {
      receiptVersion: "2.0.0",
      outputs: {
        files: [
          { path: "assets/client_bg.wasm", kind: "asset", producer: "client", bytes: 20, sha256: "b".repeat(64) },
          { path: "assets/site.css", kind: "asset", producer: "style", bytes: 10, sha256: "c".repeat(64) },
          { path: "index.html", kind: "route", producer: "/", bytes: 5, sha256: "d".repeat(64) },
          { path: "pliego.graph.json", kind: "framework", producer: "graph", bytes: 10, sha256: "e".repeat(64) },
        ],
        fileCount: 4,
        totalBytes: 45,
        sha256: "f".repeat(64),
      },
    },
  };
}

function minimalRawSample(sample) {
  return {
    sample,
    cold: {
      durationMs: 20 + sample,
      peakRssBytes: 1_000 + sample,
      record: parseBuildRecord(buildRecord()),
    },
    warm: {
      durationMs: 5 + sample,
      record: parseBuildRecord(buildRecord({
        outcome: "no-op",
        renderedArtifacts: 0,
        reusedArtifacts: 5,
        receiptBefore: "a".repeat(64),
        phases: {
          discoveryMicros: 1_000,
          clientMicros: 0,
          siteMicros: 0,
          verificationMicros: 0,
          totalMicros: 1_000,
        },
      })),
    },
    output: parseBuildLedger(buildLedger()),
  };
}
