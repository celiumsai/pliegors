#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { verify } from 'node:crypto';
import { lstat, readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import {
  CANDIDATE_KEY_FINGERPRINT,
  CANDIDATE_KEY_ID,
  CANDIDATE_PUBLIC_KEY_NAME,
  MANIFEST_NAME,
  RELEASE_MANIFEST_SCHEMA,
  REPRODUCIBILITY_SCHEMA,
  SIGNATURE_NAME,
  TARGETS,
  archiveName,
  assetRole,
  completeBundleNames,
  exactKeys,
  listRegularFiles,
  parseCliArgs,
  primaryAssetNames,
  publicKeyFingerprint,
  readCanonicalJson,
  requireCommit,
  requireExactNames,
  requireSemver,
  requireSha256,
  requireString,
  sha256File,
} from './release-bundle-lib.mjs';

export async function verifyReleaseBundle({
  directory,
  publicKeyPath,
  expectedFingerprint,
  trustedFingerprint = CANDIDATE_KEY_FINGERPRINT,
  trustedKeyId = CANDIDATE_KEY_ID,
}) {
  const actualNames = await listRegularFiles(directory);
  requireExactNames(actualNames, completeBundleNames(), 'release bundle');

  const manifestPath = path.join(directory, MANIFEST_NAME);
  const signaturePath = path.join(directory, SIGNATURE_NAME);
  const { bytes: manifestBytes, value: manifest } = await readCanonicalJson(
    manifestPath,
    MANIFEST_NAME,
  );
  exactKeys(manifest, ['assets', 'release', 'reproducibility', 'schema', 'signing'], 'manifest');
  if (manifest.schema !== RELEASE_MANIFEST_SCHEMA) throw new Error('unknown release manifest schema');

  exactKeys(manifest.release, ['commit', 'sourceDateEpoch', 'tag', 'version'], 'manifest.release');
  const version = requireSemver(manifest.release.version, 'manifest release version');
  const commit = requireCommit(manifest.release.commit, 'manifest release commit');
  if (manifest.release.tag !== `v${version}`) throw new Error('manifest tag/version mismatch');
  if (!Number.isSafeInteger(manifest.release.sourceDateEpoch) || manifest.release.sourceDateEpoch < 1) {
    throw new Error('invalid manifest sourceDateEpoch');
  }

  exactKeys(manifest.signing, ['algorithm', 'keyId', 'publicKeySha256'], 'manifest.signing');
  if (manifest.signing.algorithm !== 'Ed25519') throw new Error('unsupported signature algorithm');
  if (manifest.signing.keyId !== trustedKeyId) throw new Error('unknown candidate key ID');
  const manifestFingerprint = requireString(
    manifest.signing.publicKeySha256,
    'manifest public key fingerprint',
    /^sha256:[0-9a-f]{64}$/u,
    71,
  );
  if (manifestFingerprint !== trustedFingerprint) {
    throw new Error('manifest candidate key fingerprint drift');
  }
  if (expectedFingerprint && manifestFingerprint !== expectedFingerprint) {
    throw new Error('manifest does not match the independently expected key fingerprint');
  }

  const publicKey = await readFile(publicKeyPath);
  const actualFingerprint = publicKeyFingerprint(publicKey);
  if (actualFingerprint !== manifestFingerprint) throw new Error('public key fingerprint mismatch');
  const signatureText = await readFile(signaturePath, 'utf8');
  if (!/^[A-Za-z0-9+/]{86}==\n$/u.test(signatureText)) {
    throw new Error('detached signature is not canonical base64 Ed25519 bytes');
  }
  const signature = Buffer.from(signatureText.trim(), 'base64');
  if (signature.length !== 64 || !verify(null, manifestBytes, publicKey, signature)) {
    throw new Error('release manifest signature verification failed');
  }

  if (!Array.isArray(manifest.assets)) throw new Error('manifest assets must be an array');
  const expectedAssets = primaryAssetNames();
  requireExactNames(manifest.assets.map((asset) => asset?.name), expectedAssets, 'manifest assets');
  const sortedAssets = [...manifest.assets].sort((left, right) => left.name.localeCompare(right.name));
  if (sortedAssets.some((asset, index) => asset !== manifest.assets[index])) {
    throw new Error('manifest assets are not sorted');
  }
  for (const asset of manifest.assets) {
    exactKeys(asset, ['bytes', 'name', 'role', 'sha256'], `manifest asset ${asset.name}`);
    requireString(asset.name, 'asset name', /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/u, 128);
    if (asset.role !== assetRole(asset.name)) throw new Error(`asset role mismatch: ${asset.name}`);
    if (!Number.isSafeInteger(asset.bytes) || asset.bytes < 1) {
      throw new Error(`invalid asset byte size: ${asset.name}`);
    }
    const file = path.join(directory, asset.name);
    const stat = await lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size !== asset.bytes) {
      throw new Error(`asset size or type mismatch: ${asset.name}`);
    }
    const expectedHash = requireSha256(asset.sha256, `asset hash ${asset.name}`);
    if ((await sha256File(file)) !== expectedHash) throw new Error(`asset hash mismatch: ${asset.name}`);
  }

  exactKeys(manifest.reproducibility, ['replicasPerTarget', 'schema'], 'manifest.reproducibility');
  if (manifest.reproducibility.schema !== REPRODUCIBILITY_SCHEMA) {
    throw new Error('manifest reproducibility schema mismatch');
  }
  if (manifest.reproducibility.replicasPerTarget !== 2) {
    throw new Error('manifest requires exactly two build replicas per target');
  }

  const reproducibilityPath = path.join(directory, 'REPRODUCIBILITY.json');
  const { value: reproducibility } = await readCanonicalJson(
    reproducibilityPath,
    'REPRODUCIBILITY.json',
  );
  exactKeys(reproducibility, ['commit', 'schema', 'targets', 'version'], 'reproducibility');
  if (reproducibility.schema !== REPRODUCIBILITY_SCHEMA) throw new Error('unknown reproducibility schema');
  if (reproducibility.version !== version || reproducibility.commit !== commit) {
    throw new Error('reproducibility release identity mismatch');
  }
  if (!Array.isArray(reproducibility.targets) || reproducibility.targets.length !== TARGETS.length) {
    throw new Error('reproducibility target count mismatch');
  }
  for (let index = 0; index < TARGETS.length; index += 1) {
    const expectedTarget = TARGETS[index];
    const target = reproducibility.targets[index];
    exactKeys(target, ['binarySha256', 'replicas', 'support', 'target'], 'reproducibility target');
    if (target.target !== expectedTarget.target || target.support !== expectedTarget.support) {
      throw new Error('reproducibility target order or support mismatch');
    }
    const binaryHash = requireSha256(target.binarySha256, 'reproduced binary hash');
    if (!Array.isArray(target.replicas) || target.replicas.length !== 2) {
      throw new Error(`target ${target.target} requires two replicas`);
    }
    for (const [replicaIndex, replica] of target.replicas.entries()) {
      exactKeys(replica, ['archiveBytes', 'archiveSha256', 'binarySha256', 'replica'], 'replica');
      if (replica.replica !== replicaIndex + 1 || replica.binarySha256 !== binaryHash) {
        throw new Error(`target ${target.target} replica identity mismatch`);
      }
      requireSha256(replica.archiveSha256, 'replica archive hash');
      if (!Number.isSafeInteger(replica.archiveBytes) || replica.archiveBytes < 1) {
        throw new Error('invalid replica archive size');
      }
    }

    const archive = archiveName(target.target);
    const archiveHash = await sha256File(path.join(directory, archive));
    const sidecar = await readFile(path.join(directory, `${archive}.sha256`), 'utf8');
    if (sidecar !== `${archiveHash}  ${archive}`) throw new Error(`sidecar mismatch: ${archive}`);
    if (target.replicas[0].archiveSha256 !== archiveHash) {
      throw new Error(`selected archive is not replica 1: ${archive}`);
    }
  }

  return { version, commit, fingerprint: actualFingerprint, assets: manifest.assets.length };
}

async function main() {
  const args = parseCliArgs(process.argv.slice(2), [
    '--dir',
    '--expected-key-fingerprint',
    '--public-key',
  ]);
  if (!args['--dir']) throw new Error('--dir is required');
  const directory = path.resolve(args['--dir']);
  const publicKeyPath = path.resolve(
    args['--public-key'] ?? path.join(directory, CANDIDATE_PUBLIC_KEY_NAME),
  );
  const result = await verifyReleaseBundle({
    directory,
    publicKeyPath,
    expectedFingerprint: args['--expected-key-fingerprint'],
  });
  console.log(
    `Release bundle PASS: v${result.version} ${result.commit.slice(0, 12)} ` +
      `${result.assets} signed assets ${result.fingerprint}`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
