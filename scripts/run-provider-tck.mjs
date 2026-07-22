#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import process from 'node:process';

if (process.platform === 'win32') {
  throw new Error('the provider TCK builds Linux/OCI artifacts; run it in WSL or Linux');
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const example = path.join(root, 'examples', 'provider-tck');
const target = path.join(root, 'target', 'provider-tck');
const cargoTarget = path.join(target, 'cargo');
const r1 = path.join(target, 'r1');
const r2 = path.join(target, 'r2');
const revision = process.env.GITHUB_SHA ?? run('git', ['rev-parse', 'HEAD']).trim();
assert.match(revision, /^[0-9a-f]{40}$/u);
await mkdir(target, { recursive: true });

run('npm', ['run', 'build'], { cwd: example });
run('cargo', [
  'build', '-p', 'provider-tck', '--release', '--locked', '--bin', 'provider-tck-pack',
], { env: { ...process.env, CARGO_TARGET_DIR: cargoTarget } });
run('cargo', [
  'build', '-p', 'provider-tck', '--release', '--locked', '--bin', 'provider-tck-native',
  '--target', 'x86_64-unknown-linux-musl',
], { env: { ...process.env, CARGO_TARGET_DIR: cargoTarget } });
run('cargo', [
  'build', '-p', 'pliego-cli', '--release', '--locked', '--bin', 'pliego',
], { env: { ...process.env, CARGO_TARGET_DIR: cargoTarget } });

const executable = path.join(
  cargoTarget, 'x86_64-unknown-linux-musl', 'release', 'provider-tck-native',
);
const packer = path.join(cargoTarget, 'release', 'provider-tck-pack');
const cli = path.join(cargoTarget, 'release', 'pliego');
const worker = path.join(example, 'build');
const r1Receipt = pack('provider-tck-r1', '1', r1);
const r2Receipt = pack('provider-tck-r2', '2', r2, 'provider-tck-r1');
const r1Manifest = path.join(r1, 'pliego.pboc.json');
const r2Manifest = path.join(r2, 'pliego.pboc.json');

const admissions = [];
for (const [bundle, manifest] of [[r1, r1Manifest], [r2, r2Manifest]]) {
  run(cli, ['pboc', 'validate', manifest, '--root', bundle]);
  for (const host of ['native', 'cloudflare']) {
    admissions.push(JSON.parse(run(cli, [
      'pboc', 'admit', manifest, '--host', host, '--root', bundle,
    ])));
  }
}
const rolling = JSON.parse(run(cli, [
  'pboc', 'compatibility', r1Manifest, r2Manifest, '--direction', 'rolling',
]));
const rollback = JSON.parse(run(cli, [
  'pboc', 'compatibility', r2Manifest, r1Manifest, '--direction', 'rollback',
]));

const incompatible = JSON.parse(await readFile(r2Manifest, 'utf8'));
incompatible.compatibility.stateSchema = 'state-v2';
const incompatiblePath = path.join(target, 'incompatible-state.pboc.json');
await writeFile(incompatiblePath, `${JSON.stringify(incompatible)}\n`);
expectFailure(
  cli,
  ['pboc', 'compatibility', r1Manifest, incompatiblePath, '--direction', 'rolling'],
  'PLG-PBOC-103',
);
const unsupported = JSON.parse(await readFile(r2Manifest, 'utf8'));
unsupported.capabilities.push({ id: 'unsupported.feature', version: 1, required: true });
const unsupportedPath = path.join(target, 'unsupported-feature.pboc.json');
await writeFile(unsupportedPath, `${JSON.stringify(unsupported)}\n`);
expectFailure(cli, ['pboc', 'admit', unsupportedPath, '--host', 'cloudflare'], 'unsupported.feature@1');

const sentinel = `pliego-provider-secret-${randomUUID()}`;
const secretBoundary = JSON.parse(run(process.execPath, [
  path.join(root, 'scripts', 'check-pboc-bundle.mjs'), r2Manifest, r2,
], { env: { ...process.env, PLIEGO_TCK_SENTINEL_SECRET: sentinel } }));

const services = [];
const containerName = `pliegors-provider-tck-${process.pid}`;
try {
  services.push(service(executable, [], {
    name: 'native-r1',
    env: { ...process.env, PLIEGO_PBOC_ROOT: r1, PLIEGO_ADDR: '127.0.0.1:4330' },
  }));
  services.push(service(executable, [], {
    name: 'native-r2',
    env: { ...process.env, PLIEGO_PBOC_ROOT: r2, PLIEGO_ADDR: '127.0.0.1:4331' },
  }));
  const devWorker = path.join(example, 'scripts', 'dev-worker.mjs');
  services.push(service(process.execPath, [devWorker, r1Manifest], {
    name: 'cloudflare-r1', cwd: example,
    env: { ...process.env, PLIEGO_CF_PORT: '8788', PLIEGO_CF_NAME: 'pliegors-provider-tck-r1' },
  }));
  services.push(service(process.execPath, [devWorker, r2Manifest], {
    name: 'cloudflare-r2', cwd: example,
    env: { ...process.env, PLIEGO_CF_PORT: '8789', PLIEGO_CF_NAME: 'pliegors-provider-tck-r2' },
  }));
  await Promise.all([
    ready('http://127.0.0.1:4330/health'), ready('http://127.0.0.1:4331/health'),
    ready('http://127.0.0.1:8788/health'), ready('http://127.0.0.1:8789/health'),
  ]);

  const tckR1 = checkPair('http://127.0.0.1:4330', 'http://127.0.0.1:8788', r1Manifest, r1Receipt.manifestSha256);
  const tckR2 = checkPair('http://127.0.0.1:4331', 'http://127.0.0.1:8789', r2Manifest, r2Receipt.manifestSha256);
  const skew = JSON.parse(run(process.execPath, [
    path.join(root, 'scripts', 'check-provider-skew.mjs'),
    '--old-origin', 'http://127.0.0.1:4330', '--new-origin', 'http://127.0.0.1:8789',
    '--old-manifest', r1Manifest, '--new-manifest', r2Manifest,
    '--old-sha', r1Receipt.manifestSha256, '--new-sha', r2Receipt.manifestSha256,
  ]));
  const rollbackReplay = checkPair(
    'http://127.0.0.1:4330', 'http://127.0.0.1:8788', r1Manifest, r1Receipt.manifestSha256,
  );

  const image = `pliegors/provider-tck:${process.pid}`;
  run('docker', ['build', '--file', path.join(example, 'Containerfile'), '--tag', image, r2]);
  const imageUser = run('docker', ['image', 'inspect', image, '--format', '{{.Config.User}}']).trim();
  assert.equal(imageUser, '65532:65532', 'OCI image is not pinned to the nonroot identity');
  run('docker', [
    'run', '--detach', '--rm', '--name', containerName, '--read-only', '--cap-drop=ALL',
    '--security-opt=no-new-privileges', '--tmpfs', '/tmp:rw,noexec,nosuid,nodev,size=16m',
    '--publish', '127.0.0.1:4332:4330', image,
  ]);
  await ready('http://127.0.0.1:4332/health');
  const ociTck = checkPair(
    'http://127.0.0.1:4332', 'http://127.0.0.1:8789', r2Manifest, r2Receipt.manifestSha256,
  );

  const evidence = {
    contract: 'dev.pliegors.g3-provider-evidence/v1', revision,
    bundles: { r1: r1Receipt, r2: r2Receipt }, admissions, rolling, rollback,
    negativeCases: ['PLG-PBOC-103', 'unsupported.feature@1'], secretBoundary,
    conformance: { r1: tckR1, r2: tckR2, rollingSkew: skew, rollbackReplay, oci: ociTck },
    oci: { user: imageUser, readOnly: true, capabilitiesDropped: 'ALL', noNewPrivileges: true },
  };
  await writeFile(path.join(target, 'evidence.json'), `${JSON.stringify(evidence, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify(evidence)}\n`);
} finally {
  spawnSync('docker', ['rm', '--force', containerName], { stdio: 'ignore' });
  for (const child of services.reverse()) child.kill('SIGTERM');
}

function pack(release, sequence, output, previous) {
  const arguments_ = [executable, worker, output, release, sequence, revision];
  if (previous) arguments_.push(previous);
  return JSON.parse(run(packer, arguments_));
}

function checkPair(native, cloudflare, manifest, sha) {
  return JSON.parse(run(process.execPath, [
    path.join(root, 'scripts', 'check-provider-tck.mjs'),
    '--native', native, '--cloudflare', cloudflare,
    '--manifest', manifest, '--pboc-sha256', sha,
  ]));
}

function run(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, {
    cwd: options.cwd ?? root,
    env: options.env ?? process.env,
    encoding: 'utf8',
    stdio: options.stdio ?? ['ignore', 'pipe', 'pipe'],
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} exited ${result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  }
  return result.stdout ?? '';
}

function expectFailure(command, arguments_, token) {
  const result = spawnSync(command, arguments_, { cwd: root, encoding: 'utf8' });
  assert.notEqual(result.status, 0, `${command} unexpectedly accepted negative fixture`);
  assert.match(`${result.stdout}${result.stderr}`, new RegExp(escape(token), 'u'));
}

function service(command, arguments_, options) {
  const output = createWriteStream(path.join(target, `${options.name}.log`));
  const child = spawn(command, arguments_, {
    cwd: options.cwd ?? root, env: options.env ?? process.env, stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout.pipe(output);
  child.stderr.pipe(output);
  child.on('exit', (code) => {
    if (code && code !== 143) process.stderr.write(`${options.name} exited ${code}\n`);
  });
  return child;
}

async function ready(url) {
  for (let attempt = 0; attempt < 160; attempt += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`service did not become ready: ${url}`);
}

function escape(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
}
