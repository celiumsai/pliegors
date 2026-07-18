// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const buildPath = path.resolve(root, process.argv[2] ?? 'target/benchmarks/p8-build.json');
const browserPath = path.resolve(root, process.argv[3] ?? 'target/benchmarks/p8-browser.json');
const outputPath = path.resolve(root, process.argv[4] ?? 'target/benchmarks/p8-report.json');
const build = JSON.parse(await readFile(buildPath, 'utf8'));
const browser = JSON.parse(await readFile(browserPath, 'utf8'));

assert.equal(build.contract, 'dev.pliegors.p8-build-benchmark/v1');
assert.equal(browser.contract, 'dev.pliegors.p8-browser-benchmark/v1');
assert.match(build.revision, /^[0-9a-f]{40}$/u);
assert.equal(browser.revision, build.revision, 'build and browser reports must measure the same revision');
assert.equal(build.sourceTreeDirty, false, 'build report must come from a clean source tree');
assert.equal(browser.sourceTreeDirty, false, 'browser report must come from a clean source tree');
assert.ok(build.sampleCount >= 5, 'build report requires at least five samples');
assert.equal(build.rawObservations.length, build.sampleCount);
assert.ok(browser.rawObservations.length >= 10, 'browser report requires at least ten update samples');
assert.ok(browser.memory.rawObservations.length >= 4, 'browser report requires warmup plus at least three memory batches');

const buildMetrics = ['cleanColdBuildMs', 'noChangeWarmMs', 'contentOnlyMs', 'cssOnlyMs', 'rustViewMs'];
for (const metric of buildMetrics) {
  const values = build.rawObservations.map((entry) => finiteNonnegative(entry[metric], metric));
  assert.deepEqual(build.summary[metric], summarize(values, 'Ms'), `${metric} summary does not match raw observations`);
}
const totals = browser.rawObservations.map((entry) => finiteNonnegative(entry.totalMs, 'totalMs'));
const perUpdate = browser.rawObservations.map((entry) => finiteNonnegative(entry.perUpdateUs, 'perUpdateUs'));
assert.deepEqual(browser.updateSummary.totalMs, summarize(totals, 'Ms'));
assert.deepEqual(browser.updateSummary.perUpdateUs, summarize(perUpdate, 'Us'));
for (const observation of browser.memory.rawObservations) {
  assert.ok(Number.isSafeInteger(observation.linearMemoryBytes) && observation.linearMemoryBytes >= 0);
  assert.ok(Number.isSafeInteger(observation.domChildNodes) && observation.domChildNodes >= 0);
}

const report = {
  contract: 'dev.pliegors.p8-benchmark-report/v1',
  revision: build.revision,
  sourceTreeDirty: false,
  createdAt: new Date().toISOString(),
  percentileMethod: 'nearest-rank',
  methodologyDocument: 'docs/39-reproducible-benchmarks.md',
  build,
  browser,
  conclusions: {
    memoryPlateauObserved: browser.memory.plateau,
    evidenceScope: 'Recorded observations for one revision and the declared environments; not a universal performance guarantee.',
    competitiveComparison: 'none',
  },
  limitations: [
    'The two report sections may be captured on different operating systems and are interpreted independently.',
    'Raw observations are authoritative; p50 and p95 are nearest-rank summaries.',
    'This report does not contain or imply competitor benchmarks.',
  ],
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
assert.doesNotMatch(serialized, /\b(?:next\.js|astro|vite|remix|sveltekit|nuxt)\b/iu, 'benchmark evidence must not contain competitor marketing');
const schema = JSON.parse(await readFile(path.join(root, 'schemas', 'pliego.benchmark-report.schema.json'), 'utf8'));
const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);
const validate = ajv.compile(schema);
assert.equal(validate(report), true, `invalid merged benchmark report: ${ajv.errorsText(validate.errors)}`);
await mkdir(path.dirname(outputPath), { recursive: true });
await writeFile(outputPath, serialized, 'utf8');
process.stdout.write(`P8 benchmark report: ${outputPath}\n`);

function finiteNonnegative(value, label) {
  assert.ok(Number.isFinite(value) && value >= 0, `${label} must be finite and nonnegative`);
  return value;
}

function summarize(values, suffix) {
  const ordered = [...values].sort((left, right) => left - right);
  return {
    [`p50${suffix}`]: round(ordered[Math.ceil(ordered.length * 0.50) - 1]),
    [`p95${suffix}`]: round(ordered[Math.ceil(ordered.length * 0.95) - 1]),
  };
}

function round(value) {
  return Math.round(value * 1_000_000) / 1_000_000;
}
