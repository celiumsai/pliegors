// SPDX-License-Identifier: Apache-2.0

import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { lstat, readFile, readdir } from 'node:fs/promises';
import path from 'node:path';

export const ATTESTATION_MANIFEST = 'ATTESTATIONS.json';
export const ATTESTATION_SIGNATURE = 'ATTESTATIONS.sigstore.json';
export const SBOM_NAME = 'PLIEGORS.cdx.json';
export const PROVENANCE_NAME = 'PLIEGORS.intoto.jsonl';
export const ATTESTATION_SCHEMA = 'dev.pliegors.supply-chain-attestations/v1';
export const PROVENANCE_TYPE = 'https://in-toto.io/Statement/v1';
export const PROVENANCE_PREDICATE = 'https://slsa.dev/provenance/v1';
export const RELEASE_BUILD_TYPE = 'https://pliegors.dev/build-types/release/v1';
export const MAX_ATTESTATION_BYTES = 5 * 1024 * 1024;

export function canonicalJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

export function jsonLine(value) {
  return `${JSON.stringify(value)}\n`;
}

export function exactKeys(value, expected, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${label} keys must be exactly: ${wanted.join(', ')}`);
  }
}

export function requireString(value, label, pattern, maxBytes = 512) {
  if (typeof value !== 'string' || Buffer.byteLength(value) > maxBytes || !pattern.test(value)) {
    throw new Error(`invalid ${label}`);
  }
  return value;
}

export function requireSha256(value, label) {
  return requireString(value, label, /^[0-9a-f]{64}$/u, 64);
}

export function requireCommit(value) {
  return requireString(value, 'commit', /^[0-9a-f]{40}$/u, 40);
}

export function requireSemver(value) {
  return requireString(
    value,
    'version',
    /^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/u,
    128,
  );
}

export async function assertRegularFile(file, label = path.basename(file)) {
  const stat = await lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size < 1) {
    throw new Error(`${label} must be a non-empty regular file`);
  }
  return stat;
}

export async function sha256File(file) {
  await assertRegularFile(file);
  const hash = createHash('sha256');
  await new Promise((resolve, reject) => {
    const stream = createReadStream(file);
    stream.on('data', (chunk) => hash.update(chunk));
    stream.on('error', reject);
    stream.on('end', resolve);
  });
  return hash.digest('hex');
}

export async function readJson(file, label = path.basename(file)) {
  const stat = await assertRegularFile(file, label);
  if (stat.size > MAX_ATTESTATION_BYTES) throw new Error(`${label} exceeds size limit`);
  const bytes = await readFile(file);
  const text = bytes.toString('utf8');
  if (!Buffer.from(text, 'utf8').equals(bytes)) throw new Error(`${label} is not UTF-8`);
  try {
    return { bytes, value: JSON.parse(text) };
  } catch (error) {
    throw new Error(`${label} is invalid JSON: ${error.message}`);
  }
}

export async function regularFileNames(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const names = [];
  for (const entry of entries) {
    if (!entry.isFile() || entry.isSymbolicLink()) {
      throw new Error(`unsupported attestation entry: ${entry.name}`);
    }
    names.push(entry.name);
  }
  return names.sort((left, right) => left.localeCompare(right));
}

export function parseArgs(argv, allowed) {
  const result = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (!allowed.includes(name) || value === undefined || value.startsWith('--')) {
      throw new Error(`invalid or incomplete option: ${name ?? '<missing>'}`);
    }
    if (Object.hasOwn(result, name)) throw new Error(`duplicate option: ${name}`);
    result[name] = value;
  }
  return result;
}

