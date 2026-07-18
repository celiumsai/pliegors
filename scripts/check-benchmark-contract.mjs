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
assert.equal(typeof ajv.compile(schema), 'function');

const [buildSource, browserSource, mergeSource] = await Promise.all(files.map((file) => readFile(path.join(root, file), 'utf8')));
for (const metric of ['cleanColdBuildMs', 'noChangeWarmMs', 'contentOnlyMs', 'cssOnlyMs', 'rustViewMs']) {
  assert.match(buildSource, new RegExp(metric, 'u'));
  assert.match(mergeSource, new RegExp(metric, 'u'));
}
assert.match(buildSource, /refusing benchmark evidence from a dirty tree/u);
assert.match(browserSource, /run_browser_benchmark/u);
assert.match(browserSource, /memory\.plateau/u);
assert.match(mergeSource, /nearest-rank/u);
assert.match(mergeSource, /competitor benchmarks/u);

console.log('P8 benchmark contract PASS: build mutations, browser apply, memory plateau, and clean-revision evidence');
