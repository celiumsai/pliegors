#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import process from 'node:process';

const options = parse(process.argv.slice(2));
const manifest = JSON.parse(await readFile(options.manifest, 'utf8'));
const cases = [
  { method: 'GET', path: '/', status: 200, route: 'home' },
  { method: 'GET', path: '/api/hello/Mario', status: 200, route: 'hello' },
  { method: 'GET', path: '/asset.txt', status: 200, route: 'asset', static: true },
  { method: 'GET', path: '/health', status: 200, route: 'health' },
  { method: 'GET', path: '/stream', status: 200, route: 'stream', stream: true },
  { method: 'GET', path: '/missing', status: 404 },
  { method: 'POST', path: '/api/hello/Mario', status: 405 },
];

const receipts = [];
for (const test of cases) {
  const native = await request(options.native, test);
  const cloudflare = await request(options.cloudflare, test);
  assert.equal(native.status, test.status, `native ${test.method} ${test.path} status`);
  assert.equal(cloudflare.status, test.status, `cloudflare ${test.method} ${test.path} status`);
  assert.equal(native.body, cloudflare.body, `${test.method} ${test.path} body differs`);
  for (const header of ['content-type', 'cache-control', 'x-content-type-options', 'allow']) {
    assert.equal(
      native.headers[header] ?? null,
      cloudflare.headers[header] ?? null,
      `${test.method} ${test.path} ${header} differs`,
    );
  }
  if (test.route) {
    assert.equal(native.headers['x-pliego-route'], test.route, `native route header ${test.path}`);
    assert.equal(cloudflare.headers['x-pliego-route'], test.route, `cloudflare route header ${test.path}`);
  }
  if (!test.static && test.status === 200) {
    for (const response of [native, cloudflare]) {
      assert.equal(response.headers['x-pliego-release'], manifest.build.releaseId);
      assert.equal(response.headers['x-pliego-pboc'], options.pbocSha256);
    }
  }
  if (test.stream) {
    assert.equal(native.headers['content-length'], undefined, 'native stream was buffered');
    assert.equal(cloudflare.headers['content-length'], undefined, 'Cloudflare stream was buffered');
  }
  receipts.push({
    method: test.method,
    path: test.path,
    status: test.status,
    bodySha256: await sha256(native.body),
  });
}

process.stdout.write(`${JSON.stringify({
  contract: 'dev.pliegors.provider-tck/v1',
  pbocSha256: options.pbocSha256,
  releaseId: manifest.build.releaseId,
  providers: ['cloudflare-workers', 'native-linux-oci'],
  cases: receipts,
})}\n`);

async function request(origin, test) {
  const response = await fetch(new URL(test.path, origin), {
    method: test.method,
    redirect: 'manual',
  });
  return {
    status: response.status,
    headers: Object.fromEntries(response.headers.entries()),
    body: await response.text(),
  };
}

async function sha256(value) {
  const bytes = new TextEncoder().encode(value);
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(digest)]
    .map((byte) => byte.toString(16).padStart(2, '0'))
    .join('');
}

function parse(arguments_) {
  const values = new Map();
  for (let index = 0; index < arguments_.length; index += 2) {
    values.set(arguments_[index], arguments_[index + 1]);
  }
  for (const name of ['--native', '--cloudflare', '--manifest', '--pboc-sha256']) {
    assert.ok(values.get(name), `missing ${name}`);
  }
  return {
    native: values.get('--native'),
    cloudflare: values.get('--cloudflare'),
    manifest: values.get('--manifest'),
    pbocSha256: values.get('--pboc-sha256'),
  };
}
