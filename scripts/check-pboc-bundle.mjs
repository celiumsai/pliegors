#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import Ajv2020 from 'ajv/dist/2020.js';

const [manifestPath, root] = process.argv.slice(2);
assert.ok(manifestPath && root, 'usage: check-pboc-bundle <manifest> <bundle-root>');
const manifestBytes = await readFile(manifestPath);
const manifest = JSON.parse(manifestBytes);
const schema = JSON.parse(await readFile(new URL('../schemas/pliego.pboc.schema.json', import.meta.url)));
const validate = new Ajv2020({ allErrors: true, strict: true }).compile(schema);
assert.equal(validate(manifest), true, JSON.stringify(validate.errors));

const forbiddenFields = ['accountId', 'apiToken', 'secretValue', 'providerCredentials'];
const manifestText = manifestBytes.toString('utf8');
for (const field of forbiddenFields) assert.ok(!manifestText.includes(`"${field}"`), `PBOC contains ${field}`);

const sentinel = process.env.PLIEGO_TCK_SENTINEL_SECRET;
assert.ok(sentinel && sentinel.length >= 16, 'PLIEGO_TCK_SENTINEL_SECRET must contain at least 16 bytes');
const needle = Buffer.from(sentinel);
const files = await collect(path.resolve(root));
for (const file of files) {
  assert.equal((await readFile(file)).indexOf(needle), -1, `provider secret leaked into ${file}`);
}

process.stdout.write(`${JSON.stringify({
  contract: 'dev.pliegors.pboc-secret-boundary/v1',
  artifactCount: manifest.artifacts.length,
  scannedFileCount: files.length,
  providerFieldsAbsent: true,
  sentinelAbsent: true,
})}\n`);

async function collect(directory) {
  const output = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`symlink is forbidden: ${target}`);
    if (entry.isDirectory()) output.push(...await collect(target));
    else if (entry.isFile()) output.push(target);
  }
  return output.sort();
}
