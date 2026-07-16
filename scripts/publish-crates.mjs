// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { statSync } from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const mode = process.argv[2] ?? '--check';
assert.ok(['--check', '--publish'].includes(mode), 'usage: publish-crates.mjs --check|--publish');

const layers = [
  [
    'pliego-artifact', 'pliego-assets', 'pliego-content', 'pliego-inspect',
    'pliego-log', 'pliego-macros', 'pliego-reactive', 'pliego-starters',
  ],
  ['pliego-dom', 'pliego-fold', 'pliego-hyphae'],
  ['pliego-adapters', 'pliego-resume', 'pliego-ssg'],
  ['pliego-cli'],
];
const ordered = layers.flat();
const metadata = JSON.parse(run('cargo', ['metadata', '--no-deps', '--format-version', '1']));
const packages = new Map(metadata.packages.map((pkg) => [pkg.name, pkg]));
const version = packages.get('pliego-cli')?.version;
assert.match(version ?? '', /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u, 'invalid workspace version');
assert.deepEqual(
  ordered.toSorted(),
  [...packages.values()]
    .filter((pkg) => pkg.manifest_path.replaceAll('\\', '/').includes('/crates/'))
    .map((pkg) => pkg.name)
    .toSorted(),
  'publication order must cover every framework crate exactly once',
);

for (const name of ordered) {
  const pkg = packages.get(name);
  assert.equal(pkg.version, version, `${name} version drift`);
  assert.deepEqual(pkg.publish, ['crates-io'], `${name} registry allowlist`);
  for (const dependency of pkg.dependencies.filter((item) => item.name.startsWith('pliego-'))) {
    assert.equal(dependency.req, `=${version}`, `${name} -> ${dependency.name} must be exact`);
    assert.ok(dependency.path, `${name} -> ${dependency.name} must retain a workspace path`);
  }
}

if (mode === '--check') {
  let checked = 0;
  let deferred = 0;
  for (const name of ordered) {
    const pkg = packages.get(name);
    const internalDependencies = pkg.dependencies
      .filter((item) => item.name.startsWith('pliego-'))
      .map((item) => item.name);
    if (internalDependencies.length > 0) {
      deferred += 1;
      console.log(`defer ${name}: first-party dependencies are not indexed yet`);
      continue;
    }
    runLive('cargo', ['package', '--locked', '--allow-dirty', '--no-verify', '-p', name]);
    const archive = path.join(root, 'target', 'package', `${name}-${version}.crate`);
    const bytes = statSync(archive).size;
    assert.ok(bytes <= 10_000_000, `${name} package exceeds the crates.io 10 MB limit`);
    console.log(`package ${name}: ${bytes} bytes`);
    runLive('cargo', ['publish', '--dry-run', '--locked', '--allow-dirty', '-p', name]);
    checked += 1;
  }
  console.log(
    `Crates publication check PASS: ${checked} package(s) verified, ` +
    `${deferred} dependency-gated package(s) deferred @ ${version}`,
  );
  process.exit(0);
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
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    const result = spawnSync('cargo', ['publish', '--locked', '-p', name], {
      cwd: root,
      stdio: 'inherit',
      env: process.env,
    });
    if (result.error) throw result.error;
    if (result.status === 0) return;
    if (await registryVersion(name, expectedVersion) === 'ours') return;
    if (attempt < 3) {
      console.warn(`retry ${name}: registry dependencies may still be converging`);
      await new Promise((resolve) => setTimeout(resolve, attempt * 15_000));
    }
  }
  throw new Error(`cargo publish -p ${name} failed after three attempts`);
}
