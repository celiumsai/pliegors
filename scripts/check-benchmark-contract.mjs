// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const files = [
  'scripts/measure-p8-builds.mjs',
  'scripts/measure-browser-benchmark.mjs',
  'scripts/merge-p8-benchmark-report.mjs',
];
for (const file of files) {
  const checked = spawnSync(process.execPath, ['--check', path.join(root, file)], { encoding: 'utf8', windowsHide: true });
  assert.equal(checked.status, 0, `${file} syntax failure: ${checked.stderr}`);
}

const schema = JSON.parse(await readFile(path.join(root, 'schemas', 'pliego.benchmark-report.schema.json'), 'utf8'));
const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);
const validate = ajv.compile(schema);
assert.equal(typeof validate, 'function');

const baselinePath = path.join(root, 'benchmarks', 'baselines', 'p8-888b892.json');
const baseline = JSON.parse(await readFile(baselinePath, 'utf8'));
assert.equal(validate(baseline), true, `invalid published P8 baseline: ${ajv.errorsText(validate.errors)}`);
assert.equal(baseline.revision, '888b8929951c724b3b5146073897918779c539d1');
assert.equal(baseline.sourceTreeDirty, false);
for (const metric of ['cleanColdBuildMs', 'noChangeWarmMs', 'contentOnlyMs', 'cssOnlyMs', 'rustViewMs']) {
  const values = baseline.build.rawObservations.map((observation) => observation[metric]);
  assert.deepEqual(baseline.build.summary[metric], nearestRankSummary(values, 'Ms'));
}
assert.deepEqual(
  baseline.browser.updateSummary.perUpdateUs,
  nearestRankSummary(baseline.browser.rawObservations.map((observation) => observation.perUpdateUs), 'Us'),
);
const memoryTail = baseline.browser.memory.rawObservations.slice(-3).map((observation) => observation.linearMemoryBytes);
const observedPlateau = memoryTail.length === 3
  && memoryTail.every((value) => value === memoryTail[0])
  && baseline.browser.memory.rawObservations.every((observation) => observation.domChildNodes === 0);
assert.equal(baseline.browser.memory.plateau, observedPlateau);

const [buildSource, browserSource, mergeSource] = await Promise.all(files.map((file) => readFile(path.join(root, file), 'utf8')));
for (const metric of ['cleanColdBuildMs', 'noChangeWarmMs', 'contentOnlyMs', 'cssOnlyMs', 'rustViewMs']) {
  assert.match(buildSource, new RegExp(metric, 'u'));
  assert.match(mergeSource, new RegExp(metric, 'u'));
}
assert.match(buildSource, /refusing benchmark evidence from a dirty tree/u);
assert.match(buildSource, /writeJsonAtomic/u);
assert.match(buildSource, /checkpoint does not match this revision, sample count, or environment/u);
assert.match(browserSource, /run_browser_benchmark/u);
assert.match(browserSource, /memory\.plateau/u);
assert.match(mergeSource, /nearest-rank/u);
assert.match(mergeSource, /competitor benchmarks/u);

console.log('P8 benchmark contract PASS: build mutations, browser apply, memory plateau, and clean-revision evidence');

function nearestRankSummary(values, suffix) {
  const ordered = [...values].sort((left, right) => left - right);
  return {
    [`p50${suffix}`]: round(ordered[Math.ceil(ordered.length * 0.50) - 1]),
    [`p95${suffix}`]: round(ordered[Math.ceil(ordered.length * 0.95) - 1]),
  };
}

function round(value) {
  return Math.round(value * 1_000_000) / 1_000_000;
}
