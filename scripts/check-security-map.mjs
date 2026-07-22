#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const maps = [
  {
    gate: 'G1',
    path: 'security/asvs-v5.0.0-g1.json',
    minimum: 15,
    required: [
      'v5.0.0-1.1.1',
      'v5.0.0-4.2.1',
      'v5.0.0-15.2.2',
      'v5.0.0-16.2.5',
      'v5.0.0-16.5.1',
    ],
  },
  {
    gate: 'G2',
    path: 'security/asvs-v5.0.0-g2.json',
    minimum: 20,
    required: [
      'v5.0.0-1.3.6',
      'v5.0.0-3.5.1',
      'v5.0.0-5.3.2',
      'v5.0.0-7.2.4',
      'v5.0.0-7.4.1',
      'v5.0.0-8.3.1',
      'v5.0.0-14.2.2',
      'v5.0.0-16.2.5',
    ],
  },
];

const statuses = new Set(['verified', 'partial', 'shared', 'not-applicable']);
let controls = 0;
for (const definition of maps) {
  const mapPath = path.join(root, definition.path);
  const controlMap = JSON.parse(await readFile(mapPath, 'utf8'));
  assert.equal(controlMap.schema, 'dev.pliegors.asvs-control-map/v1');
  assert.equal(controlMap.standard, 'OWASP ASVS');
  assert.equal(controlMap.standardVersion, '5.0.0');
  assert.equal(controlMap.targetLevel, 2);
  assert.match(controlMap.disclaimer, /not an application compliance claim/u);
  assert.equal(new Set(controlMap.controls.map(({ id }) => id)).size, controlMap.controls.length);
  assert.ok(
    controlMap.controls.length >= definition.minimum,
    `${definition.gate} map is unexpectedly shallow`,
  );

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

  for (const required of definition.required) {
    assert.ok(
      controlMap.controls.some(({ id }) => id === required),
      `missing required ${definition.gate} control ${required}`,
    );
  }
  assert.ok(
    controlMap.controls.some(({ status }) => status !== 'verified'),
    `${definition.gate} map makes an implausible universal verification claim`,
  );
  controls += controlMap.controls.length;
}

console.log(`Security map check passed (${maps.length} gates, ${controls} scoped controls).`);
