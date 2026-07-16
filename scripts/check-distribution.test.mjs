// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const githubReleaseBase = 'https://github.com/celiumsai/pliegors/releases/download';

test('GitHub draft release contract is self-verifying', () => {
  const result = spawnSync(process.execPath, ['scripts/check-distribution.mjs'], {
    cwd: root,
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /5 targets x 2 replicas/u);
  assert.match(result.stdout, /signed release candidate/u);
  assert.match(result.stdout, /gated manual draft/u);
});

test('installers require an explicit GitHub release selector', async () => {
  const [shell, powershell] = await Promise.all([
    readFile(path.join(root, 'scripts', 'install.sh'), 'utf8'),
    readFile(path.join(root, 'scripts', 'install.ps1'), 'utf8'),
  ]);

  for (const [name, source] of [
    ['install.sh', shell],
    ['install.ps1', powershell],
  ]) {
    assert.ok(source.includes(githubReleaseBase), `${name} lacks the GitHub release base`);
    assert.doesNotMatch(source, /api\.github\.com|latest\.txt|dl\.pliego\.run/u);
    assert.match(source, /release selector is required/iu);
    assert.match(source, /channel latest/iu);
    assert.match(source, /\.sha256/u);
    assert.match(source, /\[0-9a-f\]\{64\}|\[0-9a-f\]\{64\}/iu);
    assert.match(source, /sha256 mismatch/iu);
  }

  assert.match(shell, /releases\/download"/u);
  assert.match(shell, /base="\$download_base\/v\$version"/u);
  assert.match(shell, /releases\/latest\/download/u);
  assert.match(powershell, /\$base = "\$downloadBase\/v\$Version"/u);
  assert.match(powershell, /releases\/latest\/download/u);
  assert.match(shell, /archive="pliego-\$target\.zip"/u);
  assert.match(powershell, /\$archive = "pliego-\$target\.zip"/u);
  assert.doesNotMatch(shell, /archive="pliego-\$version-/u);
  assert.doesNotMatch(powershell, /\$archive = "pliego-\$Version-/u);
});

test('Unix installer covers the supported release target matrix', async () => {
  const shell = await readFile(path.join(root, 'scripts', 'install.sh'), 'utf8');
  for (const target of [
    'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-gnu',
    'x86_64-apple-darwin',
    'aarch64-apple-darwin',
  ]) {
    assert.ok(shell.includes(target), `install.sh lacks ${target}`);
  }
});

test('Windows installer is x64-only and verifies before extraction', async () => {
  const source = await readFile(path.join(root, 'scripts', 'install.ps1'), 'utf8');
  assert.match(source, /Unsupported Windows architecture/u);
  assert.match(source, /x86_64-pc-windows-msvc/u);
  assert.ok(source.indexOf('$actual -ne $expected') < source.indexOf('Expand-Archive'));
});

test(
  'PowerShell installer fails closed on a checksum mismatch',
  { skip: process.platform !== 'win32' },
  async () => {
    const fixture = await mkdtemp(path.join(tmpdir(), 'pliegors-installer-'));
    try {
      const archive = path.join(fixture, 'pliego-local.zip');
      const installDir = path.join(fixture, 'install');
      await writeFile(archive, 'not a release archive');
      await writeFile(`${archive}.sha256`, `${'0'.repeat(64)}  pliego-local.zip`);

      const result = spawnSync(
        'pwsh.exe',
        [
          '-NoLogo',
          '-NoProfile',
          '-NonInteractive',
          '-ExecutionPolicy',
          'Bypass',
          '-File',
          path.join(root, 'scripts', 'install.ps1'),
          '-ArchivePath',
          archive,
          '-InstallDir',
          installDir,
        ],
        { encoding: 'utf8' },
      );

      assert.notEqual(result.status, 0);
      assert.match(`${result.stdout}\n${result.stderr}`, /sha256 mismatch/iu);
    } finally {
      await rm(fixture, { recursive: true, force: true });
    }
  },
);
