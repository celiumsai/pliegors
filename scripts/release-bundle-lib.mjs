// SPDX-License-Identifier: Apache-2.0

import { createHash, createPublicKey } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { lstat, readFile, readdir } from 'node:fs/promises';
import path from 'node:path';

export const CANDIDATE_KEY_ID = 'pliegors-candidate-2026-01';
export const CANDIDATE_KEY_FINGERPRINT =
  'sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250';
export const CANDIDATE_PUBLIC_KEY_NAME = 'PLIEGORS-CANDIDATE-RELEASE.pub.pem';
export const MANIFEST_NAME = 'RELEASE-MANIFEST.json';
export const SIGNATURE_NAME = 'RELEASE-MANIFEST.json.sig';
export const RELEASE_MANIFEST_SCHEMA = 'dev.pliegors.release-manifest/v1';
export const REPRODUCIBILITY_SCHEMA = 'dev.pliegors.release-reproducibility/v1';
export const BUILD_METADATA_SCHEMA = 'dev.pliegors.candidate-build/v1';
export const MAX_JSON_BYTES = 1024 * 1024;

export const TARGETS = Object.freeze([
  Object.freeze({ target: 'aarch64-apple-darwin', support: 'development' }),
  Object.freeze({ target: 'aarch64-unknown-linux-gnu', support: 'production' }),
  Object.freeze({ target: 'x86_64-apple-darwin', support: 'development' }),
  Object.freeze({ target: 'x86_64-pc-windows-msvc', support: 'development' }),
  Object.freeze({ target: 'x86_64-unknown-linux-gnu', support: 'production' }),
]);

export function archiveName(target) {
  return `pliego-${target}.zip`;
}

export function primaryAssetNames() {
  const names = [];
  for (const { target } of TARGETS) {
    const archive = archiveName(target);
    names.push(archive, `${archive}.sha256`);
  }
  names.push(
    'install.ps1',
    'install.sh',
    'release-bundle-lib.mjs',
    'REPRODUCIBILITY.json',
    'verify-release-bundle.mjs',
  );
  return names.sort((left, right) => left.localeCompare(right));
}

export function completeBundleNames() {
  return [
    ...primaryAssetNames(),
    CANDIDATE_PUBLIC_KEY_NAME,
    MANIFEST_NAME,
    SIGNATURE_NAME,
  ].sort((left, right) => left.localeCompare(right));
}

export function assetRole(name) {
  if (name.endsWith('.zip')) return 'cli-archive';
  if (name.endsWith('.zip.sha256')) return 'integrity-sidecar';
  if (name === 'install.ps1' || name === 'install.sh') return 'installer';
  if (name === 'REPRODUCIBILITY.json') return 'reproducibility-evidence';
  if (name === 'verify-release-bundle.mjs') return 'bundle-verifier';
  if (name === 'release-bundle-lib.mjs') return 'bundle-verifier-library';
  throw new Error(`unknown release asset role: ${name}`);
}

export function canonicalJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
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

export function requireCommit(value, label = 'commit') {
  return requireString(value, label, /^[0-9a-f]{40}$/u, 40);
}

export function requireSemver(value, label = 'version') {
  return requireString(
    value,
    label,
    /^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/u,
    128,
  );
}

export async function assertRegularFile(file, label = file) {
  const stat = await lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink()) {
    throw new Error(`${label} must be a regular file`);
  }
  return stat;
}

export async function sha256File(file) {
  await assertRegularFile(file);
  const hash = createHash('sha256');
  await new Promise((resolve, reject) => {
    const input = createReadStream(file);
    input.on('data', (chunk) => hash.update(chunk));
    input.on('error', reject);
    input.on('end', resolve);
  });
  return hash.digest('hex');
}

export async function readCanonicalJson(file, label = path.basename(file)) {
  const stat = await assertRegularFile(file, label);
  if (stat.size > MAX_JSON_BYTES) throw new Error(`${label} exceeds ${MAX_JSON_BYTES} bytes`);
  const bytes = await readFile(file);
  const text = bytes.toString('utf8');
  if (!Buffer.from(text, 'utf8').equals(bytes)) throw new Error(`${label} is not UTF-8`);
  let value;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new Error(`${label} is invalid JSON: ${error.message}`);
  }
  if (canonicalJson(value) !== text) throw new Error(`${label} is not canonical JSON`);
  return { bytes, value };
}

export async function listRegularFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if (!entry.isFile() || entry.isSymbolicLink()) {
      throw new Error(`release bundle contains non-file entry: ${entry.name}`);
    }
    files.push(entry.name);
  }
  return files.sort((left, right) => left.localeCompare(right));
}

export function requireExactNames(actual, expected, label) {
  const sorted = [...actual].sort((left, right) => left.localeCompare(right));
  const wanted = [...expected].sort((left, right) => left.localeCompare(right));
  if (sorted.length !== wanted.length || sorted.some((name, index) => name !== wanted[index])) {
    throw new Error(`${label} exact set mismatch; expected ${wanted.join(', ')}, got ${sorted.join(', ')}`);
  }
}

export function publicKeyFingerprint(publicKey) {
  const key = publicKey?.type === 'public' ? publicKey : createPublicKey(publicKey);
  const der = key.export({ type: 'spki', format: 'der' });
  return `sha256:${createHash('sha256').update(der).digest('hex')}`;
}

export function parseCliArgs(argv, allowed) {
  const result = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!allowed.includes(option) || value === undefined || value.startsWith('--')) {
      throw new Error(`invalid or incomplete option: ${option ?? '<missing>'}`);
    }
    if (Object.hasOwn(result, option)) throw new Error(`duplicate option: ${option}`);
    result[option] = value;
  }
  return result;
}
