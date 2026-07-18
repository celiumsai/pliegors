#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { access, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

const KEY_FINGERPRINT = 'sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250';
const SOURCE_ARCHIVE = 'pliegors-source.tar.gz';
const MAX_OUTPUT_BYTES = 32 * 1024;
const options = parseArguments(process.argv.slice(2));
const release = path.resolve(options.release);
const output = path.resolve(options.output);
const target = options.target;
const scenario = options.scenario;
const dependencySource = options.dependencySource;
const environmentId = options.environmentId;
const nativeTarget = hostTarget();

if (target !== nativeTarget) throw new Error(`target ${target} cannot run on ${nativeTarget}`);
if (output === release || output.startsWith(`${release}${path.sep}`)) {
  throw new Error('golden report output must be disjoint from the release bundle');
}

const releaseManifestPath = path.join(release, 'RELEASE-MANIFEST.json');
const manifest = JSON.parse(await readFile(releaseManifestPath, 'utf8'));
const releaseManifestSha256 = await sha256File(releaseManifestPath);
const version = manifest.release?.version;
const revision = manifest.release?.commit;
assert.match(version ?? '', /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u, 'invalid release version');
assert.match(revision ?? '', /^[0-9a-f]{40}$/u, 'invalid release revision');

const createdAt = new Date().toISOString();
const work = await mkdtemp(path.join(os.tmpdir(), 'pliegors-golden-'));
const installRoot = path.join(work, 'install');
const sourceParent = path.join(work, 'source');
const sourceRoot = path.join(sourceParent, 'pliegors-source');
const project = projectPath(work, scenario);
const cli = path.join(installRoot, 'bin', process.platform === 'win32' ? 'pliego.exe' : 'pliego');
const steps = [];
let failure = null;
let doctor;
let reproductionBundleSha256 = null;

try {
  await step('verify-release-bundle', () => run(process.execPath, [
    path.join(release, 'verify-release-bundle.mjs'),
    '--dir', release,
    '--expected-key-fingerprint', KEY_FINGERPRINT,
  ], work));

  if (dependencySource === 'candidate-source') {
    await step('extract-signed-source', async () => {
      const archive = path.join(release, SOURCE_ARCHIVE);
      const listing = await run('tar', ['-tzf', archive], work);
      const entries = listing.stdout.split(/\r?\n/u).filter(Boolean);
      if (entries.length === 0 || entries.length > 10_000) throw new Error('source archive entry count is invalid');
      for (const entry of entries) {
        if (!entry.startsWith('pliegors-source/')
          || entry.includes('\\')
          || entry.split('/').includes('..')) {
          throw new Error(`unsafe source archive entry: ${entry}`);
        }
      }
      await mkdir(sourceParent, { recursive: true });
      await run('tar', ['-xzf', archive, '-C', sourceParent], work);
      await access(path.join(sourceRoot, 'Cargo.toml'));
      await assertMissing(path.join(sourceRoot, '.git'));
    });
  }

  await step('install', () => install(release, target, installRoot));
  await step('version', async () => {
    const result = await run(cli, ['version'], work);
    if (result.stdout.trim() !== `pliego ${version}`) throw new Error('installed CLI version disagrees with signed release');
  });
  await step('telemetry-default-before', async () => {
    const status = JSON.parse((await run(cli, ['telemetry', 'status', '--format', 'json'], work)).stdout);
    if (status.contract !== 'dev.pliegors.telemetry-status/v1'
      || status.enabled !== false
      || status.localEventCount !== 0
      || status.networkSubmission !== 'none') {
      throw new Error('telemetry is not disabled and empty before first use');
    }
  });
  await step('doctor-global', async () => {
    const result = await run(cli, ['doctor', '--format', 'json'], work);
    doctor = JSON.parse(result.stdout);
    if (doctor.reportVersion !== '1.0.0' || doctor.cliVersion !== version || doctor.summary?.failed !== 0) {
      throw new Error('global doctor report is unhealthy or has an unknown contract');
    }
  });
  await step('new', async () => {
    const arguments_ = ['new', project];
    if (dependencySource === 'candidate-source') arguments_.push('--framework-path', sourceRoot);
    await run(cli, arguments_, work);
    await validateScaffold(path.join(project, 'Cargo.toml'), dependencySource, version, sourceRoot);
  });
  await step('check', () => run(cli, ['check'], project));
  await step('cargo-test', () => run('cargo', ['test', '--locked'], project));
  await step('dev-smoke', () => devSmoke(cli, project));
  await step('build', () => run(cli, ['build'], project));
  await step('inspect', () => run(cli, ['inspect'], project));
  await step('why-artifact', () => run(cli, ['why', 'artifact', '/'], project));
  await step('report-bundle', async () => {
    const bundle = path.join(project, 'target', 'p8-golden-report.tar');
    await run(cli, ['report', '--bundle', '--output', bundle], project);
    reproductionBundleSha256 = await sha256File(bundle);
  });
  await step('upgrade-check', async () => {
    const result = await run(cli, ['upgrade', '--check', '--target', version, '--format', 'json'], project);
    const report = JSON.parse(result.stdout);
    if (report.reportVersion !== '1.0.0' || report.targetVersion !== version || report.status !== 'compatible') {
      throw new Error('upgrade report does not confirm exact-version compatibility');
    }
  });
  await step('doctor-project', async () => {
    const report = JSON.parse((await run(cli, ['doctor', '--format', 'json'], project)).stdout);
    if (report.project === null || report.summary?.failed !== 0) throw new Error('project doctor report is unhealthy');
  });
  await step('telemetry-default-after', async () => {
    const status = JSON.parse((await run(cli, ['telemetry', 'status', '--format', 'json'], project)).stdout);
    if (status.enabled !== false || status.localEventCount !== 0 || status.networkSubmission !== 'none') {
      throw new Error('first-use path changed disabled telemetry state');
    }
  });
} catch (error) {
  failure = bounded(String(error?.stack ?? error));
} finally {
  try {
    await step('uninstall', () => uninstall(release, installRoot));
    await assertMissing(cli);
  } catch (error) {
    failure ??= bounded(String(error?.stack ?? error));
  }

  const report = {
    contract: 'dev.pliegors.p8-golden-path/v1',
    version,
    revision,
    environmentId,
    target,
    scenario,
    dependencySource,
    createdAt,
    completedAt: new Date().toISOString(),
    host: {
      platform: process.platform,
      architecture: process.arch,
      osRelease: os.release(),
      cpuModel: os.cpus()[0]?.model?.trim() ?? 'unknown',
      logicalCpuCount: os.cpus().length,
      totalMemoryBytes: os.totalmem(),
      node: process.version,
      rustc: commandText('rustc', ['--version']),
      cargo: commandText('cargo', ['--version']),
    },
    workspace: {
      pathLength: project.length,
      containsUnicode: /[^\x00-\x7F]/u.test(project),
      exceedsLegacyWindowsMaxPath: project.length > 260,
    },
    cliSha256: doctor?.host?.executableSha256 ?? null,
    releaseManifestSha256,
    reproductionBundleSha256,
    steps,
    passed: failure === null,
    failure,
  };
  await mkdir(path.dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  await rm(work, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}

if (failure) throw new Error(`golden path failed; report: ${output}\n${failure}`);
process.stdout.write(`P8 golden path PASS: ${target} ${scenario} ${dependencySource}\n`);

function parseArguments(argv) {
  const allowed = new Set(['--release', '--output', '--target', '--scenario', '--dependency-source', '--environment-id']);
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!allowed.has(option) || !value || value.startsWith('--') || Object.hasOwn(values, option)) {
      throw new Error(`invalid or incomplete option: ${option ?? '<missing>'}`);
    }
    values[option] = value;
  }
  for (const required of allowed) if (!values[required]) throw new Error(`${required} is required`);
  if (!['standard', 'unicode', 'long-path'].includes(values['--scenario'])) throw new Error('unknown golden scenario');
  if (!['candidate-source', 'registry'].includes(values['--dependency-source'])) throw new Error('unknown dependency source');
  if (!/^[a-z0-9][a-z0-9-]{0,63}$/u.test(values['--environment-id'])) throw new Error('invalid environment ID');
  return {
    release: values['--release'],
    output: values['--output'],
    target: values['--target'],
    scenario: values['--scenario'],
    dependencySource: values['--dependency-source'],
    environmentId: values['--environment-id'],
  };
}

function hostTarget() {
  const key = `${process.platform}/${process.arch}`;
  const targets = new Map([
    ['linux/x64', 'x86_64-unknown-linux-gnu'],
    ['linux/arm64', 'aarch64-unknown-linux-gnu'],
    ['darwin/x64', 'x86_64-apple-darwin'],
    ['darwin/arm64', 'aarch64-apple-darwin'],
    ['win32/x64', 'x86_64-pc-windows-msvc'],
  ]);
  const target_ = targets.get(key);
  if (!target_) throw new Error(`unsupported golden host: ${key}`);
  return target_;
}

function projectPath(work, selectedScenario) {
  if (selectedScenario === 'unicode') return path.join(work, 'edición-東京', 'application');
  if (selectedScenario === 'long-path') {
    let current = path.join(work, 'long-path');
    let index = 0;
    while (current.length <= 300) {
      current = path.join(current, `segment-${String(index).padStart(2, '0')}-abcdefghijkl`);
      index += 1;
    }
    return path.join(current, 'application');
  }
  return path.join(work, 'application');
}

async function step(name, action) {
  const start = process.hrtime.bigint();
  try {
    await action();
    steps.push({ name, status: 'pass', durationMs: elapsedMilliseconds(start) });
  } catch (error) {
    steps.push({ name, status: 'fail', durationMs: elapsedMilliseconds(start) });
    throw error;
  }
}

function elapsedMilliseconds(start) {
  return Math.round(Number(process.hrtime.bigint() - start) / 1_000) / 1_000;
}

async function install(releaseDirectory, releaseTarget, destination) {
  const archive = path.join(releaseDirectory, `pliego-${releaseTarget}.zip`);
  if (process.platform === 'win32') {
    await run('pwsh', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(releaseDirectory, 'install.ps1'), '-ArchivePath', archive, '-InstallDir', destination], releaseDirectory);
  } else {
    await run('bash', [path.join(releaseDirectory, 'install.sh'), '--archive', archive, '--install-dir', destination], releaseDirectory);
  }
}

async function uninstall(releaseDirectory, destination) {
  if (process.platform === 'win32') {
    await run('pwsh', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(releaseDirectory, 'install.ps1'), '-Uninstall', '-InstallDir', destination], releaseDirectory);
  } else {
    await run('bash', [path.join(releaseDirectory, 'install.sh'), '--uninstall', '--install-dir', destination], releaseDirectory);
  }
}

async function validateScaffold(manifestPath, source, expectedVersion, expectedSourceRoot) {
  const manifest = await readFile(manifestPath, 'utf8');
  if (/\bgit\s*=/u.test(manifest)) throw new Error('scaffold contains a Git dependency');
  if (source === 'registry') {
    if (/\bpath\s*=/u.test(manifest)) throw new Error('registry scaffold contains a path dependency');
    const matches = manifest.match(new RegExp(`version = "=${escapeRegex(expectedVersion)}"`, 'gu')) ?? [];
    if (matches.length !== 4) throw new Error('registry scaffold does not pin four exact first-party dependencies');
  } else {
    const declaredPaths = [...manifest.matchAll(/path = "([^"]+)"/gu)]
      .map((match) => path.resolve(path.dirname(manifestPath), match[1]));
    const canonicalSourceRoot = await realpath(expectedSourceRoot);
    const canonicalPaths = await Promise.all(declaredPaths.map((entry) => realpath(entry)));
    if (canonicalPaths.length !== 4 || canonicalPaths.some((entry) => !isPathInside(canonicalSourceRoot, entry))) {
      throw new Error('candidate scaffold path dependencies escape the signed source archive');
    }
  }
}

function isPathInside(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative === ''
    || (relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative));
}

async function devSmoke(cliPath, projectRoot) {
  const port = await freePort();
  const child = spawn(cliPath, ['dev', String(port)], {
    cwd: projectRoot,
    env: cleanEnvironment(),
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (chunk) => { stdout = bounded(`${stdout}${chunk}`); });
  child.stderr.on('data', (chunk) => { stderr = bounded(`${stderr}${chunk}`); });
  try {
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
      if (child.exitCode !== null) throw new Error(`dev server exited ${child.exitCode}\n${stdout}\n${stderr}`);
      try {
        const response = await fetch(`http://127.0.0.1:${port}/`, { signal: AbortSignal.timeout(1_000) });
        if (response.status === 200 && (await response.text()).includes('<!doctype html>')) return;
      } catch {}
      await delay(100);
    }
    throw new Error(`dev server did not become healthy\n${stdout}\n${stderr}`);
  } finally {
    if (child.exitCode === null) child.kill();
    await Promise.race([new Promise((resolve) => child.once('exit', resolve)), delay(5_000)]);
    if (child.exitCode === null) child.kill('SIGKILL');
  }
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      server.close((error) => error ? reject(error) : resolve(address.port));
    });
  });
}

function run(command, args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: cleanEnvironment(),
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => { stdout = bounded(`${stdout}${chunk}`); });
    child.stderr.on('data', (chunk) => { stderr = bounded(`${stderr}${chunk}`); });
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (code === 0) resolve({ stdout, stderr });
      else reject(new Error(`${command} ${args.join(' ')} failed (${signal ?? code})\n${stdout}\n${stderr}`));
    });
  });
}

function cleanEnvironment() {
  const environment = { ...process.env };
  for (const name of ['CARGO_TARGET_DIR', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTFLAGS']) delete environment[name];
  environment.PLIEGO_HOME = installRoot;
  return environment;
}

function commandText(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8', windowsHide: true });
  return result.status === 0 ? result.stdout.trim() : 'unavailable';
}

async function assertMissing(file) {
  try {
    await access(file);
  } catch (error) {
    if (error?.code === 'ENOENT') return;
    throw error;
  }
  throw new Error(`path must be absent: ${path.basename(file)}`);
}

async function sha256File(file) {
  const hash = createHash('sha256');
  await new Promise((resolve, reject) => {
    const stream = createReadStream(file);
    stream.on('data', (chunk) => hash.update(chunk));
    stream.once('error', reject);
    stream.once('end', resolve);
  });
  return hash.digest('hex');
}

function bounded(value) {
  return String(value).slice(-MAX_OUTPUT_BYTES);
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
