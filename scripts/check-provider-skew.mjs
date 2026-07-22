#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import process from 'node:process';

const options = parse(process.argv.slice(2));
const oldManifest = JSON.parse(await readFile(options.oldManifest, 'utf8'));
const newManifest = JSON.parse(await readFile(options.newManifest, 'utf8'));
assert.equal(oldManifest.build.applicationId, newManifest.build.applicationId);
assert.equal(oldManifest.compatibility.epoch, newManifest.compatibility.epoch);
assert.equal(oldManifest.compatibility.stateSchema, newManifest.compatibility.stateSchema);
assert.ok(oldManifest.compatibility.sequence < newManifest.compatibility.sequence);
assert.equal(newManifest.compatibility.previousReleaseId, oldManifest.build.releaseId);
assert.equal(newManifest.compatibility.rollbackSafe, true);

const cases = [
  { method: 'GET', path: '/', status: 200, dynamic: true },
  { method: 'GET', path: '/api/hello/Mario', status: 200, dynamic: true },
  { method: 'GET', path: '/asset.txt', status: 200 },
  { method: 'GET', path: '/health', status: 200, dynamic: true },
  { method: 'GET', path: '/stream', status: 200, dynamic: true },
  { method: 'GET', path: '/missing', status: 404 },
  { method: 'POST', path: '/api/hello/Mario', status: 405 },
];
const receipts = [];
for (const test of cases) {
  const oldResponse = await request(options.oldOrigin, test);
  const newResponse = await request(options.newOrigin, test);
  assert.equal(oldResponse.status, test.status, `old ${test.method} ${test.path}`);
  assert.equal(newResponse.status, test.status, `new ${test.method} ${test.path}`);
  assert.equal(oldResponse.body, newResponse.body, `skew body ${test.method} ${test.path}`);
  for (const header of ['content-type', 'cache-control', 'x-content-type-options', 'allow']) {
    assert.equal(oldResponse.headers[header] ?? null, newResponse.headers[header] ?? null, `${header} skew`);
  }
  if (test.dynamic) {
    assertIdentity(oldResponse, oldManifest, options.oldSha);
    assertIdentity(newResponse, newManifest, options.newSha);
  }
  receipts.push({ method: test.method, path: test.path, status: test.status });
}

process.stdout.write(`${JSON.stringify({
  contract: 'dev.pliegors.provider-skew/v1',
  applicationId: oldManifest.build.applicationId,
  epoch: oldManifest.compatibility.epoch,
  stateSchema: oldManifest.compatibility.stateSchema,
  from: { releaseId: oldManifest.build.releaseId, pbocSha256: options.oldSha },
  to: { releaseId: newManifest.build.releaseId, pbocSha256: options.newSha },
  rollbackSafe: true,
  cases: receipts,
})}\n`);

function assertIdentity(response, manifest, sha) {
  assert.equal(response.headers['x-pliego-release'], manifest.build.releaseId);
  assert.equal(response.headers['x-pliego-pboc'], sha);
}

async function request(origin, test) {
  const response = await fetch(new URL(test.path, origin), { method: test.method, redirect: 'manual' });
  return {
    status: response.status,
    headers: Object.fromEntries(response.headers.entries()),
    body: await response.text(),
  };
}

function parse(arguments_) {
  const values = new Map();
  for (let index = 0; index < arguments_.length; index += 2) {
    values.set(arguments_[index], arguments_[index + 1]);
  }
  for (const name of [
    '--old-origin', '--new-origin', '--old-manifest', '--new-manifest', '--old-sha', '--new-sha',
  ]) assert.ok(values.get(name), `missing ${name}`);
  return Object.fromEntries([...values].map(([key, value]) => [
    key.slice(2).replaceAll(/-([a-z])/gu, (_, letter) => letter.toUpperCase()), value,
  ]));
}
