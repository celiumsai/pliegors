// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const reportSchema = JSON.parse(await readFile(path.join(root, 'schemas', 'pliego.golden-path-report.schema.json'), 'utf8'));
const matrixSchema = JSON.parse(await readFile(path.join(root, 'schemas', 'pliego.golden-matrix.schema.json'), 'utf8'));
const ajv = new Ajv2020({ allErrors: true, allowUnionTypes: true, strict: true });
addFormats(ajv);
const validateReport = ajv.compile(reportSchema);
const validateMatrix = ajv.compile(matrixSchema);

const environments = new Map([
  ['linux-x64', expected('x86_64-unknown-linux-gnu', 'standard', 'linux', 'x64')],
  ['linux-arm64', expected('aarch64-unknown-linux-gnu', 'standard', 'linux', 'arm64')],
  ['macos-x64', expected('x86_64-apple-darwin', 'standard', 'darwin', 'x64')],
  ['macos-arm64', expected('aarch64-apple-darwin', 'standard', 'darwin', 'arm64')],
  ['windows-x64', expected('x86_64-pc-windows-msvc', 'standard', 'win32', 'x64')],
  ['windows-unicode', expected('x86_64-pc-windows-msvc', 'unicode', 'win32', 'x64')],
  ['windows-long-path', expected('x86_64-pc-windows-msvc', 'long-path', 'win32', 'x64')],
  ['container-linux-x64', expected('x86_64-unknown-linux-gnu', 'standard', 'linux', 'x64')],
  ['wsl2-x64', expected('x86_64-unknown-linux-gnu', 'standard', 'linux', 'x64')],
]);

if (process.argv.length === 2) {
  const checked = spawnSync(process.execPath, ['--check', path.join(root, 'scripts', 'run-golden-path.mjs')], { encoding: 'utf8', windowsHide: true });
  assert.equal(checked.status, 0, checked.stderr);
  const runner = await readFile(path.join(root, 'scripts', 'run-golden-path.mjs'), 'utf8');
  for (const command of ['telemetry-default-before', 'doctor-global', 'cargo-test', 'dev-smoke', 'report-bundle', 'upgrade-check', 'doctor-project', 'telemetry-default-after', 'uninstall']) {
    assert.match(runner, new RegExp(`step\\('${command}'`, 'u'));
  }
  assert.match(runner, /candidate-source/u);
  assert.match(runner, /registry/u);
  assert.match(runner, /pliegors-source\.tar\.gz/u);
  assert.match(runner, /await realpath\(expectedSourceRoot\)/u);
  console.log('P8 golden path contract PASS: schemas and release-only runner');
  process.exit(0);
}

const arguments_ = parseArguments(process.argv.slice(2));
const expectedDependencySource = arguments_.mode === 'candidate' ? 'candidate-source' : 'registry';
const reportFiles = await collectReports(arguments_.input);
if (arguments_.wslReport) reportFiles.push(path.resolve(arguments_.wslReport));
const expectedIds = new Set([...environments.keys()].filter((id) => id !== 'wsl2-x64'));
if (arguments_.wslReport) expectedIds.add('wsl2-x64');
if (arguments_.mode === 'draft' && !arguments_.wslReport) throw new Error('draft promotion requires --wsl-report');
assert.equal(reportFiles.length, expectedIds.size, 'golden report count mismatch');

const reports = [];
const observedIds = new Set();
const releaseManifestHashes = new Set();
for (const file of reportFiles) {
  const bytes = await readFile(file);
  const report = JSON.parse(bytes.toString('utf8'));
  assert.equal(validateReport(report), true, `${file}: ${ajv.errorsText(validateReport.errors)}`);
  assert.ok(expectedIds.has(report.environmentId), `unexpected golden environment: ${report.environmentId}`);
  assert.ok(!observedIds.has(report.environmentId), `duplicate golden environment: ${report.environmentId}`);
  observedIds.add(report.environmentId);
  const contract = environments.get(report.environmentId);
  assert.deepEqual(
    [report.target, report.scenario, report.host.platform, report.host.architecture],
    [contract.target, contract.scenario, contract.platform, contract.architecture],
    `${report.environmentId} environment contract drift`,
  );
  assert.equal(report.version, arguments_.version);
  assert.equal(report.revision, arguments_.commit);
  assert.equal(report.dependencySource, expectedDependencySource);
  assert.equal(report.passed, true);
  assert.equal(report.failure, null);
  assert.match(report.cliSha256, /^[0-9a-f]{64}$/u);
  assert.match(report.releaseManifestSha256, /^[0-9a-f]{64}$/u);
  releaseManifestHashes.add(report.releaseManifestSha256);
  assert.match(report.reproductionBundleSha256, /^[0-9a-f]{64}$/u);
  assert.deepEqual(report.steps.map((step) => step.name), requiredSteps(expectedDependencySource));
  assert.ok(report.steps.every((step) => step.status === 'pass'));
  if (report.scenario === 'unicode') assert.equal(report.workspace.containsUnicode, true);
  if (report.scenario === 'long-path') assert.equal(report.workspace.exceedsLegacyWindowsMaxPath, true);
  if (report.environmentId === 'wsl2-x64') assert.match(report.host.osRelease, /microsoft.*wsl2/iu);
  reports.push({
    environmentId: report.environmentId,
    target: report.target,
    scenario: report.scenario,
    hostPlatform: report.host.platform,
    hostArchitecture: report.host.architecture,
    reportSha256: createHash('sha256').update(bytes).digest('hex'),
    cliSha256: report.cliSha256,
    releaseManifestSha256: report.releaseManifestSha256,
    reproductionBundleSha256: report.reproductionBundleSha256,
    totalDurationMs: Math.round(report.steps.reduce((total, step) => total + step.durationMs, 0) * 1_000) / 1_000,
  });
}
assert.deepEqual(observedIds, expectedIds);
assert.equal(releaseManifestHashes.size, 1, 'golden reports do not verify one exact release manifest');
reports.sort((left, right) => left.environmentId.localeCompare(right.environmentId));
const matrix = {
  contract: 'dev.pliegors.p8-golden-matrix/v1',
  version: arguments_.version,
  revision: arguments_.commit,
  mode: arguments_.mode,
  dependencySource: expectedDependencySource,
  createdAt: new Date().toISOString(),
  complete: observedIds.has('wsl2-x64'),
  reports,
  limitations: [
    'GitHub-hosted rows use clean ephemeral runners; WSL2 requires a separately captured local report.',
    'Candidate mode resolves first-party crates from the signed source archive; draft mode requires the exact crates.io version.',
    'A passing matrix is evidence for one release revision and the recorded runner images, not every possible host.',
  ],
};
assert.equal(validateMatrix(matrix), true, ajv.errorsText(validateMatrix.errors));
await mkdir(path.dirname(arguments_.output), { recursive: true });
await writeFile(arguments_.output, `${JSON.stringify(matrix, null, 2)}\n`, 'utf8');
console.log(`P8 golden matrix PASS: ${reports.length} environments, complete=${matrix.complete}`);

function expected(target, scenario, platform, architecture) {
  return { target, scenario, platform, architecture };
}

function requiredSteps(source) {
  return [
    'verify-release-bundle',
    ...(source === 'candidate-source' ? ['extract-signed-source'] : []),
    'install', 'version', 'telemetry-default-before', 'doctor-global', 'new', 'check',
    'cargo-test', 'dev-smoke', 'build', 'inspect', 'why-artifact', 'report-bundle',
    'upgrade-check', 'doctor-project', 'telemetry-default-after', 'uninstall',
  ];
}

function parseArguments(argv) {
  const allowed = new Set(['--input', '--output', '--commit', '--version', '--mode', '--wsl-report']);
  const result = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!allowed.has(option) || !value || value.startsWith('--') || Object.hasOwn(result, option)) {
      throw new Error(`invalid or incomplete option: ${option ?? '<missing>'}`);
    }
    result[option] = value;
  }
  for (const required of ['--input', '--output', '--commit', '--version', '--mode']) {
    if (!result[required]) throw new Error(`${required} is required`);
  }
  assert.match(result['--commit'], /^[0-9a-f]{40}$/u);
  assert.match(result['--version'], /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u);
  assert.ok(['candidate', 'draft'].includes(result['--mode']));
  return {
    input: path.resolve(result['--input']),
    output: path.resolve(result['--output']),
    commit: result['--commit'],
    version: result['--version'],
    mode: result['--mode'],
    wslReport: result['--wsl-report'],
  };
}

async function collectReports(directory) {
  const queue = [path.resolve(directory)];
  const files = [];
  let entries = 0;
  while (queue.length > 0) {
    const current = queue.shift();
    for (const entry of await readdir(current, { withFileTypes: true })) {
      entries += 1;
      if (entries > 100) throw new Error('golden report input exceeds entry bound');
      const child = path.join(current, entry.name);
      const stat = await lstat(child);
      if (stat.isSymbolicLink()) throw new Error(`golden report input contains a link: ${entry.name}`);
      if (entry.isDirectory()) queue.push(child);
      else if (entry.isFile() && entry.name.endsWith('.json')) files.push(child);
      else throw new Error(`unsupported golden report input: ${entry.name}`);
    }
  }
  return files.sort((left, right) => left.localeCompare(right));
}
