#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import {
  createPrivateKey,
  createPublicKey,
  sign,
} from 'node:crypto';
import { lstat, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import {
  CANDIDATE_KEY_FINGERPRINT,
  CANDIDATE_KEY_ID,
  CANDIDATE_PUBLIC_KEY_NAME,
  MANIFEST_NAME,
  RELEASE_MANIFEST_SCHEMA,
  REPRODUCIBILITY_SCHEMA,
  SIGNATURE_NAME,
  assetRole,
  canonicalJson,
  listRegularFiles,
  parseCliArgs,
  primaryAssetNames,
  publicKeyFingerprint,
  requireCommit,
  requireExactNames,
  requireSemver,
  sha256File,
} from './release-bundle-lib.mjs';
import { verifyReleaseBundle } from './verify-release-bundle.mjs';

async function main() {
  const args = parseCliArgs(process.argv.slice(2), [
    '--commit',
    '--dir',
    '--source-date-epoch',
    '--version',
  ]);
  for (const required of ['--commit', '--dir', '--source-date-epoch', '--version']) {
    if (!args[required]) throw new Error(`${required} is required`);
  }
  const directory = path.resolve(args['--dir']);
  const version = requireSemver(args['--version']);
  const commit = requireCommit(args['--commit']);
  const sourceDateEpoch = Number(args['--source-date-epoch']);
  if (!Number.isSafeInteger(sourceDateEpoch) || sourceDateEpoch < 1) {
    throw new Error('source date epoch must be a positive safe integer');
  }
  const privatePem = process.env.PLIEGORS_CANDIDATE_SIGNING_KEY;
  if (!privatePem || Buffer.byteLength(privatePem) > 16 * 1024) {
    throw new Error('PLIEGORS_CANDIDATE_SIGNING_KEY is missing or oversized');
  }

  const publicKeyPath = path.join(directory, CANDIDATE_PUBLIC_KEY_NAME);
  const publicPem = await readFile(publicKeyPath);
  if (publicKeyFingerprint(publicPem) !== CANDIDATE_KEY_FINGERPRINT) {
    throw new Error('candidate public key fingerprint drift');
  }
  const privateKey = createPrivateKey(privatePem);
  const derivedPublic = createPublicKey(privateKey);
  if (publicKeyFingerprint(derivedPublic) !== CANDIDATE_KEY_FINGERPRINT) {
    throw new Error('candidate private key does not match the committed public key');
  }

  requireExactNames(
    await listRegularFiles(directory),
    [...primaryAssetNames(), CANDIDATE_PUBLIC_KEY_NAME],
    'unsigned release bundle',
  );
  const assets = [];
  for (const name of primaryAssetNames()) {
    const file = path.join(directory, name);
    const stat = await lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size < 1) {
      throw new Error(`invalid release asset: ${name}`);
    }
    assets.push({ name, role: assetRole(name), bytes: stat.size, sha256: await sha256File(file) });
  }
  const manifest = {
    schema: RELEASE_MANIFEST_SCHEMA,
    release: { version, tag: `v${version}`, commit, sourceDateEpoch },
    signing: {
      algorithm: 'Ed25519',
      keyId: CANDIDATE_KEY_ID,
      publicKeySha256: CANDIDATE_KEY_FINGERPRINT,
    },
    reproducibility: { schema: REPRODUCIBILITY_SCHEMA, replicasPerTarget: 2 },
    assets,
  };
  const manifestBytes = Buffer.from(canonicalJson(manifest), 'utf8');
  const signature = sign(null, manifestBytes, privateKey);
  if (signature.length !== 64) throw new Error('Ed25519 signature has an unexpected length');
  await writeFile(path.join(directory, MANIFEST_NAME), manifestBytes, { flag: 'wx' });
  await writeFile(path.join(directory, SIGNATURE_NAME), `${signature.toString('base64')}\n`, {
    encoding: 'utf8',
    flag: 'wx',
  });
  const result = await verifyReleaseBundle({
    directory,
    publicKeyPath,
    expectedFingerprint: CANDIDATE_KEY_FINGERPRINT,
  });
  console.log(`Signed manifest PASS: ${result.assets} assets for v${result.version}`);
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
