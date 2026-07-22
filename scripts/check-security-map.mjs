#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const mapPath = path.join(root, 'security/asvs-v5.0.0-g1.json');
const controlMap = JSON.parse(await readFile(mapPath, 'utf8'));

assert.equal(controlMap.schema, 'dev.pliegors.asvs-control-map/v1');
assert.equal(controlMap.standard, 'OWASP ASVS');
assert.equal(controlMap.standardVersion, '5.0.0');
assert.equal(controlMap.targetLevel, 2);
assert.match(controlMap.disclaimer, /not an application compliance claim/u);
assert.equal(new Set(controlMap.controls.map(({ id }) => id)).size, controlMap.controls.length);
assert.ok(controlMap.controls.length >= 15, 'G1 map is unexpectedly shallow');

const statuses = new Set(['verified', 'partial', 'shared', 'not-applicable']);
for (const control of controlMap.controls) {
  assert.match(control.id, /^v5\.0\.0-\d+\.\d+\.\d+$/u, `invalid ASVS ID ${control.id}`);
  assert.ok(statuses.has(control.status), `invalid status for ${control.id}`);
  assert.ok(control.owner.length > 0, `missing owner for ${control.id}`);
  assert.ok(control.scopeNote.length >= 40, `missing ownership rationale for ${control.id}`);
  assert.ok(control.evidence.length > 0, `missing evidence for ${control.id}`);
  for (const relativePath of control.evidence) {
    const resolved = path.resolve(root, relativePath);
    assert.ok(resolved.startsWith(`${root}${path.sep}`), `evidence escapes repository: ${relativePath}`);
    await access(resolved);
  }
}

for (const required of [
  'v5.0.0-1.1.1',
  'v5.0.0-4.2.1',
  'v5.0.0-15.2.2',
  'v5.0.0-16.2.5',
  'v5.0.0-16.5.1',
]) {
  assert.ok(controlMap.controls.some(({ id }) => id === required), `missing required G1 control ${required}`);
}

assert.ok(
  controlMap.controls.some(({ status }) => status !== 'verified'),
  'control map makes an implausible universal verification claim',
);

console.log(`Security map check passed (${controlMap.controls.length} scoped controls).`);
