// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const mode = process.argv[2] ?? '--check';
assert.ok(['--check', '--publish'].includes(mode), 'usage: publish-crates.mjs --check|--publish');

const layers = [
  [
    'pliego-artifact', 'pliego-assets', 'pliego-content', 'pliego-inspect',
    'pliego-data', 'pliego-log', 'pliego-macros', 'pliego-reactive', 'pliego-router', 'pliego-sdk',
    'pliego-starters',
  ],
  ['pliego-dom', 'pliego-fold', 'pliego-hyphae'],
  ['pliego-adapters', 'pliego-resume', 'pliego-runtime', 'pliego-ssg'],
  ['pliego-cli'],
];
const ordered = layers.flat();
const metadata = JSON.parse(run('cargo', ['metadata', '--no-deps', '--format-version', '1']));
const packages = new Map(metadata.packages.map((pkg) => [pkg.name, pkg]));
const product = JSON.parse(readFileSync(path.join(root, 'product.capabilities.json'), 'utf8'));
const unreleased = new Set(product.framework.unreleasedCrates);
for (const name of unreleased) {
  const pkg = packages.get(name);
  assert.ok(
    pkg?.manifest_path.replaceAll('\\', '/').includes('/crates/'),
    `unknown unreleased crate: ${name}`,
  );
}
const targetDirectory = metadata.target_directory;
const version = packages.get('pliego-cli')?.version;
assert.match(version ?? '', /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u, 'invalid workspace version');
assert.deepEqual(
  ordered.toSorted(),
  [...packages.values()]
    .filter((pkg) => pkg.manifest_path.replaceAll('\\', '/').includes('/crates/'))
    .filter((pkg) => !unreleased.has(pkg.name))
    .map((pkg) => pkg.name)
    .toSorted(),
  'publication order must cover every framework crate exactly once',
);

for (const name of ordered) {
  const pkg = packages.get(name);
  assert.deepEqual(pkg.publish, ['crates-io'], `${name} registry allowlist`);
  for (const dependency of pkg.dependencies.filter((item) => item.name.startsWith('pliego-'))) {
    const dependencyPackage = packages.get(dependency.name);
    assert.ok(dependencyPackage, `${name} -> ${dependency.name} workspace package`);
    assert.equal(
      dependency.req,
      `=${dependencyPackage.version}`,
      `${name} -> ${dependency.name} must be exact`,
    );
    assert.ok(dependency.path, `${name} -> ${dependency.name} must retain a workspace path`);
  }
}

if (mode === '--check') {
  let checked = 0;
  let deferred = 0;
  const registryAvailability = new Map();
  const sourcePreviewChain = new Set(unreleased);
  for (const name of ordered) {
    const pkg = packages.get(name);
    const internalDependencies = pkg.dependencies
      .filter((item) => item.name.startsWith('pliego-'))
      .map((item) => item.name);
    const previewDependencies = internalDependencies.filter((dependency) => sourcePreviewChain.has(dependency));
    if (previewDependencies.length > 0) {
      sourcePreviewChain.add(name);
      deferred += 1;
      console.log(`defer ${name}: source-preview dependency chain: ${previewDependencies.join(', ')}`);
      continue;
    }
    const unavailableDependencies = [];
    for (const dependency of internalDependencies) {
      const dependencyVersion = packages.get(dependency).version;
      const key = `${dependency}@${dependencyVersion}`;
      if (!registryAvailability.has(key)) {
        registryAvailability.set(key, await registryVersion(dependency, dependencyVersion));
      }
      if (registryAvailability.get(key) !== 'ours') unavailableDependencies.push(key);
    }
    if (unavailableDependencies.length > 0) {
      deferred += 1;
      console.log(`defer ${name}: not indexed: ${unavailableDependencies.join(', ')}`);
      continue;
    }
    if (name === 'pliego-sdk') assertPackagedOpenSdkWit();
    runLive('cargo', ['package', '--locked', '--allow-dirty', '--no-verify', '-p', name]);
    const archive = path.join(targetDirectory, 'package', `${name}-${pkg.version}.crate`);
    const bytes = statSync(archive).size;
    assert.ok(bytes <= 10_000_000, `${name} package exceeds the crates.io 10 MB limit`);
    console.log(`package ${name} ${pkg.version}: ${bytes} bytes`);
    runLive('cargo', ['publish', '--dry-run', '--locked', '--allow-dirty', '-p', name]);
    checked += 1;
  }
  console.log(
    `Crates publication check PASS: ${checked} package(s) verified, ` +
    `${deferred} dependency-gated package(s) deferred across ` +
    `${new Set(ordered.map((name) => packages.get(name).version)).size} version family/families`,
  );
  process.exit(0);
}

assert.equal(
  unreleased.size,
  0,
  `publication is blocked until source-preview crates are promoted: ${[...unreleased].sort().join(', ')}`,
);

function assertPackagedOpenSdkWit() {
  const packagedWit = run('cargo', ['package', '--list', '--allow-dirty', '-p', 'pliego-sdk'])
    .split(/\r?\n/u)
    .map((file) => file.replaceAll('\\', '/'))
    .filter((file) => file.endsWith('.wit'))
    .toSorted();
  assert.deepEqual(packagedWit, [
    'wit/build/build.wit',
    'wit/component/component.wit',
    'wit/deploy/deploy.wit',
    'wit/diagnostics/diagnostics.wit',
    'wit/effects/effects.wit',
    'wit/http/http.wit',
    'wit/manifest/manifest.wit',
  ], 'pliego-sdk package must contain the complete normative WIT surface');
}

assert.equal(process.env.PLIEGORS_PUBLISH_CONFIRMATION, `publish:v${version}`, 'publication confirmation mismatch');
assert.ok(
  process.env.CARGO_REGISTRY_TOKEN || process.env.PLIEGORS_USE_CARGO_CREDENTIALS === '1',
  'use CARGO_REGISTRY_TOKEN or opt in to credentials from cargo login',
);
assert.equal(run('git', ['status', '--porcelain']).trim(), '', 'working tree must be clean');
assert.equal(run('git', ['branch', '--show-current']).trim(), 'main', 'publication must run from main');
assert.equal(
  run('git', ['rev-parse', 'HEAD']).trim(),
  run('git', ['rev-parse', 'origin/main']).trim(),
  'main must equal origin/main',
);
assert.ok(
  ordered.every((name) => packages.get(name).version === version),
  'publication requires every framework crate to share the confirmed CLI version',
);

for (const name of ordered) {
  const state = await registryVersion(name, version);
  if (state === 'ours') {
    console.log(`skip ${name} ${version}: already published by PliegoRS`);
    continue;
  }
  assert.equal(state, 'missing', `${name} ${version} exists with unexpected metadata`);
  await publishPackage(name, version);
  await waitForRegistry(name, version);
}
console.log(`Crates publication PASS: ${ordered.length} packages @ ${version}`);

function run(command, args) {
  return execFileSync(command, args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

function runLive(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit', env: process.env });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} ${args.join(' ')} failed`);
}

async function registryVersion(name, expectedVersion, attempt = 1) {
  let response;
  try {
    response = await fetch(`https://crates.io/api/v1/crates/${name}`, {
      headers: { 'user-agent': `PliegoRS-release/${version} (hello@pliegors.dev)` },
      signal: AbortSignal.timeout(15_000),
    });
  } catch (error) {
    if (attempt < 3) {
      await new Promise((resolve) => setTimeout(resolve, attempt * 2_000));
      return registryVersion(name, expectedVersion, attempt + 1);
    }
    throw error;
  }
  if ([429, 502, 503, 504].includes(response.status) && attempt < 3) {
    await new Promise((resolve) => setTimeout(resolve, attempt * 2_000));
    return registryVersion(name, expectedVersion, attempt + 1);
  }
  if (response.status === 404) return 'missing';
  assert.equal(response.status, 200, `${name} registry lookup returned HTTP ${response.status}`);
  const body = await response.json();
  const repository = body.crate?.repository ?? '';
  const versions = body.versions ?? [];
  if (repository !== 'https://github.com/celiumsai/pliegors') return 'other-owner';
  return versions.some((item) => item.num === expectedVersion) ? 'ours' : 'missing';
}

async function waitForRegistry(name, expectedVersion) {
  for (let attempt = 1; attempt <= 30; attempt += 1) {
    if (await registryVersion(name, expectedVersion) === 'ours') {
      console.log(`indexed ${name} ${expectedVersion}`);
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 10_000));
  }
  throw new Error(`${name} ${expectedVersion} did not appear in the crates.io API within five minutes`);
}

async function publishPackage(name, expectedVersion) {
  let ordinaryFailures = 0;
  for (let attempt = 1; attempt <= 40; attempt += 1) {
    const result = await spawnLiveCapture('cargo', ['publish', '--locked', '-p', name]);
    if (result.status === 0) return;
    if (await registryVersion(name, expectedVersion) === 'ours') return;

    const retryAt = cratesIoRetryAt(result.output);
    if (retryAt !== undefined) {
      const waitMs = Math.max(retryAt - Date.now() + 5_000, 5_000);
      console.warn(`rate-limited ${name}: waiting until ${new Date(retryAt).toISOString()}`);
      await waitWithHeartbeat(waitMs, name);
      continue;
    }

    ordinaryFailures += 1;
    if (ordinaryFailures < 3) {
      console.warn(`retry ${name}: registry dependencies may still be converging`);
      await new Promise((resolve) => setTimeout(resolve, ordinaryFailures * 15_000));
      continue;
    }
    break;
  }
  throw new Error(`cargo publish -p ${name} exhausted its guarded retry budget`);
}

function spawnLiveCapture(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: root, env: process.env, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    for (const stream of [child.stdout, child.stderr]) {
      stream.on('data', (chunk) => {
        const text = chunk.toString();
        output += text;
        if (stream === child.stdout) process.stdout.write(text);
        else process.stderr.write(text);
      });
    }
    child.once('error', reject);
    child.once('close', (status) => resolve({ status, output }));
  });
}

function cratesIoRetryAt(output) {
  const match = output.match(/Please try again after (.+? GMT) and see/u);
  if (!match) return undefined;
  const timestamp = Date.parse(match[1]);
  assert.ok(Number.isFinite(timestamp), `invalid crates.io retry timestamp: ${match[1]}`);
  return timestamp;
}

async function waitWithHeartbeat(waitMs, name) {
  const deadline = Date.now() + waitMs;
  while (Date.now() < deadline) {
    const remaining = deadline - Date.now();
    console.log(`wait ${name}: ${Math.ceil(remaining / 1_000)}s remaining`);
    await new Promise((resolve) => setTimeout(resolve, Math.min(60_000, remaining)));
  }
}
