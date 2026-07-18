// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const commit = 'a'.repeat(40);
const version = '0.0.2-canary.1';
const hosted = [
  ['linux-x64', 'x86_64-unknown-linux-gnu', 'standard', 'linux', 'x64'],
  ['linux-arm64', 'aarch64-unknown-linux-gnu', 'standard', 'linux', 'arm64'],
  ['macos-x64', 'x86_64-apple-darwin', 'standard', 'darwin', 'x64'],
  ['macos-arm64', 'aarch64-apple-darwin', 'standard', 'darwin', 'arm64'],
  ['windows-x64', 'x86_64-pc-windows-msvc', 'standard', 'win32', 'x64'],
  ['windows-unicode', 'x86_64-pc-windows-msvc', 'unicode', 'win32', 'x64'],
  ['windows-long-path', 'x86_64-pc-windows-msvc', 'long-path', 'win32', 'x64'],
  ['container-linux-x64', 'x86_64-unknown-linux-gnu', 'standard', 'linux', 'x64'],
];

test('candidate matrix validates eight hosted reports and remains explicitly incomplete', async () => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), 'pliegors-golden-matrix-'));
  try {
    const input = path.join(temporary, 'input');
    await writeReports(input, 'candidate-source');
    const output = path.join(temporary, 'matrix.json');
    const result = checkMatrix(['--input', input, '--output', output, '--commit', commit, '--version', version, '--mode', 'candidate']);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const matrix = JSON.parse(await readFile(output, 'utf8'));
    assert.equal(matrix.reports.length, 8);
    assert.equal(matrix.complete, false);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

test('draft matrix requires and validates same-revision WSL2 evidence', async () => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), 'pliegors-golden-matrix-'));
  try {
    const input = path.join(temporary, 'input');
    await writeReports(input, 'registry');
    const wsl = path.join(temporary, 'wsl.json');
    await writeFile(wsl, JSON.stringify(report('wsl2-x64', 'x86_64-unknown-linux-gnu', 'standard', 'linux', 'x64', 'registry', true)));
    const output = path.join(temporary, 'matrix.json');
    const result = checkMatrix(['--input', input, '--output', output, '--commit', commit, '--version', version, '--mode', 'draft', '--wsl-report', wsl]);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const matrix = JSON.parse(await readFile(output, 'utf8'));
    assert.equal(matrix.reports.length, 9);
    assert.equal(matrix.complete, true);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

test('matrix rejects a Unicode scenario without a Unicode workspace', async () => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), 'pliegors-golden-matrix-'));
  try {
    const input = path.join(temporary, 'input');
    await writeReports(input, 'candidate-source', true);
    const result = checkMatrix(['--input', input, '--output', path.join(temporary, 'matrix.json'), '--commit', commit, '--version', version, '--mode', 'candidate']);
    assert.notEqual(result.status, 0);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

async function writeReports(directory, source, breakUnicode = false) {
  await mkdir(directory, { recursive: true });
  for (const tuple of hosted) {
    const value = report(...tuple, source, false);
    if (breakUnicode && value.environmentId === 'windows-unicode') value.workspace.containsUnicode = false;
    await writeFile(path.join(directory, `${value.environmentId}.json`), JSON.stringify(value));
  }
}

function report(environmentId, target, scenario, platform, architecture, source, wsl) {
  const stepNames = [
    'verify-release-bundle',
    ...(source === 'candidate-source' ? ['extract-signed-source'] : []),
    'install', 'version', 'telemetry-default-before', 'doctor-global', 'new', 'check',
    'cargo-test', 'dev-smoke', 'build', 'inspect', 'why-artifact', 'report-bundle',
    'upgrade-check', 'doctor-project', 'telemetry-default-after', 'uninstall',
  ];
  return {
    contract: 'dev.pliegors.p8-golden-path/v1',
    version,
    revision: commit,
    environmentId,
    target,
    scenario,
    dependencySource: source,
    createdAt: '2026-07-18T00:00:00.000Z',
    completedAt: '2026-07-18T00:01:00.000Z',
    host: {
      platform,
      architecture,
      osRelease: wsl ? '6.18.0-microsoft-standard-WSL2' : 'fixture',
      cpuModel: 'fixture CPU',
      logicalCpuCount: 4,
      totalMemoryBytes: 8_000_000_000,
      node: 'v24.0.0',
      rustc: 'rustc 1.85.0',
      cargo: 'cargo 1.85.0',
    },
    workspace: {
      pathLength: scenario === 'long-path' ? 301 : 80,
      containsUnicode: scenario === 'unicode',
      exceedsLegacyWindowsMaxPath: scenario === 'long-path',
    },
    cliSha256: 'b'.repeat(64),
    releaseManifestSha256: 'd'.repeat(64),
    reproductionBundleSha256: 'c'.repeat(64),
    steps: stepNames.map((name) => ({ name, status: 'pass', durationMs: 1 })),
    passed: true,
    failure: null,
  };
}

function checkMatrix(arguments_) {
  return spawnSync(process.execPath, ['scripts/check-golden-matrix.mjs', ...arguments_], {
    cwd: root,
    encoding: 'utf8',
    windowsHide: true,
  });
}
