// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, utimes, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('deterministic ZIP ignores host order and timestamps while preserving exact bytes', async () => {
  const temporary = await mkdtemp(path.join(os.tmpdir(), 'pliegors-zip-'));
  try {
    const first = await fixture(path.join(temporary, 'one'), false);
    const second = await fixture(path.join(temporary, 'two'), true);
    const firstZip = path.join(temporary, 'first.zip');
    const secondZip = path.join(temporary, 'second.zip');
    createZip(first, firstZip);
    createZip(second, secondZip);
    const firstBytes = await readFile(firstZip);
    const secondBytes = await readFile(secondZip);
    assert.deepEqual(firstBytes, secondBytes);
    assert.deepEqual(readStoredEntries(firstBytes), new Map([
      ['pliego-fixture/LICENSE', 'license'],
      ['pliego-fixture/bin/pliego', 'binary'],
    ]));
    const extracted = path.join(temporary, 'extracted');
    extractZip(firstZip, extracted);
    assert.equal(await readFile(path.join(extracted, 'pliego-fixture', 'LICENSE'), 'utf8'), 'license');
    assert.equal(await readFile(path.join(extracted, 'pliego-fixture', 'bin', 'pliego'), 'utf8'), 'binary');
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

async function fixture(parent, reverse) {
  const directory = path.join(parent, 'pliego-fixture');
  await mkdir(path.join(directory, 'bin'), { recursive: true });
  const files = reverse
    ? [['bin/pliego', 'binary'], ['LICENSE', 'license']]
    : [['LICENSE', 'license'], ['bin/pliego', 'binary']];
  for (const [relative, bytes] of files) await writeFile(path.join(directory, relative), bytes);
  const timestamp = reverse ? new Date('2026-07-18T12:00:00Z') : new Date('2001-01-01T00:00:00Z');
  for (const [relative] of files) await utimes(path.join(directory, relative), timestamp, timestamp);
  return directory;
}

function createZip(directory, output) {
  const result = spawnSync(process.execPath, [
    'scripts/create-deterministic-zip.mjs',
    '--root', directory,
    '--output', output,
    '--executable', 'pliego-fixture/bin/pliego',
  ], { cwd: root, encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
}

function extractZip(archive, destination) {
  const command = process.platform === 'win32' ? 'pwsh' : 'unzip';
  const arguments_ = process.platform === 'win32'
    ? ['-NoProfile', '-Command', 'Expand-Archive -LiteralPath $env:PLIEGO_ZIP -DestinationPath $env:PLIEGO_ZIP_DESTINATION']
    : ['-q', archive, '-d', destination];
  const result = spawnSync(command, arguments_, {
    encoding: 'utf8',
    env: { ...process.env, PLIEGO_ZIP: archive, PLIEGO_ZIP_DESTINATION: destination },
    windowsHide: true,
  });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
}

function readStoredEntries(bytes) {
  const entries = new Map();
  let offset = 0;
  while (bytes.readUInt32LE(offset) === 0x04034b50) {
    assert.equal(bytes.readUInt16LE(offset + 8), 0);
    const size = bytes.readUInt32LE(offset + 18);
    const nameLength = bytes.readUInt16LE(offset + 26);
    const extraLength = bytes.readUInt16LE(offset + 28);
    const nameStart = offset + 30;
    const dataStart = nameStart + nameLength + extraLength;
    const name = bytes.subarray(nameStart, nameStart + nameLength).toString('utf8');
    entries.set(name, bytes.subarray(dataStart, dataStart + size).toString('utf8'));
    offset = dataStart + size;
  }
  assert.equal(bytes.readUInt32LE(offset), 0x02014b50);
  return entries;
}
