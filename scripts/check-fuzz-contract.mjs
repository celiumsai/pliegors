// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const targets = [
  'portable_path',
  'project_manifest',
  'event_schema',
  'snapshot',
  'dom_adoption',
  'adapter_manifest',
];
const corpus = new Map([
  ['portable_path', ['valid.txt', 'traversal.txt']],
  ['project_manifest', ['valid.toml', 'overlap.toml']],
  ['event_schema', ['valid.json', 'duplicate.json']],
  ['snapshot', ['raw.txt']],
  ['dom_adoption', ['parser-sensitive.txt']],
  ['adapter_manifest', ['valid-shape.txt']],
]);

const [manifest, runner, workflow] = await Promise.all([
  readFile(path.join(root, 'fuzz', 'Cargo.toml'), 'utf8'),
  readFile(path.join(root, 'scripts', 'run-fuzz.sh'), 'utf8'),
  readFile(path.join(root, '.github', 'workflows', 'fuzz.yml'), 'utf8'),
]);

for (const target of targets) {
  assert.match(manifest, new RegExp(`name = "${target}"`, 'u'));
  assert.match(runner, new RegExp(`(?:^|\\s)${target}(?:\\s|$|\\|)`, 'mu'));
  for (const file of corpus.get(target)) {
    await access(path.join(root, 'fuzz', 'corpus', target, file));
  }
}

assert.match(manifest, /libfuzzer-sys = "=0\.4\.13"/u);
assert.match(runner, /nightly-2026-06-26/u);
assert.match(runner, /FUZZ_RUNS/u);
assert.match(runner, /mktemp -d/u);
assert.match(workflow, /CARGO_FUZZ_VERSION: 0\.13\.2/u);
assert.match(workflow, /FUZZ_RUNS: 512/u);
assert.match(workflow, /scripts\/run-fuzz\.sh/u);
assert.doesNotMatch(workflow, /pull_request_target/u);

console.log(`Fuzz contract PASS: ${targets.length} targets with reviewed seed corpora`);
