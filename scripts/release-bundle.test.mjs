// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { generateKeyPairSync, sign } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  BUILD_METADATA_SCHEMA,
  CANDIDATE_PUBLIC_KEY_NAME,
  MANIFEST_NAME,
  RELEASE_MANIFEST_SCHEMA,
  REPRODUCIBILITY_SCHEMA,
  SIGNATURE_NAME,
  SOURCE_ARCHIVE_NAME,
  TARGETS,
  archiveName,
  assetRole,
  canonicalJson,
  primaryAssetNames,
  publicKeyFingerprint,
  sha256File,
} from './release-bundle-lib.mjs';
import { verifyReleaseBundle } from './verify-release-bundle.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function temporaryDirectory() {
  return mkdtemp(path.join(tmpdir(), 'pliegors-release-test-'));
}

async function writeSignedFixture() {
  const directory = await temporaryDirectory();
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const fingerprint = publicKeyFingerprint(publicKey);
  const keyId = 'pliegors-test-key';
  const binaryHash = 'b'.repeat(64);
  const targets = [];
  for (const target of TARGETS) {
    const archive = archiveName(target.target);
    await writeFile(path.join(directory, archive), `archive:${target.target}`);
    const archiveHash = await sha256File(path.join(directory, archive));
    const bytes = Buffer.byteLength(`archive:${target.target}`);
    await writeFile(path.join(directory, `${archive}.sha256`), `${archiveHash}  ${archive}`);
    targets.push({
      target: target.target,
      support: target.support,
      binarySha256: binaryHash,
      replicas: [1, 2].map((replica) => ({
        replica,
        binarySha256: binaryHash,
        archiveSha256: replica === 1 ? archiveHash : 'c'.repeat(64),
        archiveBytes: bytes,
      })),
    });
  }
  for (const name of ['install.ps1', 'install.sh', 'run-golden-path.mjs', 'release-bundle-lib.mjs', 'source-archive-listing.mjs', SOURCE_ARCHIVE_NAME, 'verify-release-bundle.mjs']) {
    await writeFile(path.join(directory, name), `fixture:${name}\n`);
  }
  await writeFile(
    path.join(directory, 'REPRODUCIBILITY.json'),
    canonicalJson({ schema: REPRODUCIBILITY_SCHEMA, version: '0.0.1', commit: 'a'.repeat(40), targets }),
  );
  await writeFile(
    path.join(directory, CANDIDATE_PUBLIC_KEY_NAME),
    publicKey.export({ type: 'spki', format: 'pem' }),
  );
  const assets = [];
  for (const name of primaryAssetNames()) {
    const bytes = await readFile(path.join(directory, name));
    assets.push({ name, role: assetRole(name), bytes: bytes.length, sha256: await sha256File(path.join(directory, name)) });
  }
  const manifest = {
    schema: RELEASE_MANIFEST_SCHEMA,
    release: { version: '0.0.1', tag: 'v0.0.1', commit: 'a'.repeat(40), sourceDateEpoch: 1 },
    signing: { algorithm: 'Ed25519', keyId, publicKeySha256: fingerprint },
    reproducibility: { schema: REPRODUCIBILITY_SCHEMA, replicasPerTarget: 2 },
    assets,
  };
  const manifestBytes = Buffer.from(canonicalJson(manifest));
  await writeFile(path.join(directory, MANIFEST_NAME), manifestBytes);
  await writeFile(path.join(directory, SIGNATURE_NAME), `${sign(null, manifestBytes, privateKey).toString('base64')}\n`);
  return { directory, fingerprint, keyId };
}

test('signed release bundle verifies its exact assets and independent fingerprint', async () => {
  const fixture = await writeSignedFixture();
  try {
    const result = await verifyReleaseBundle({
      directory: fixture.directory,
      publicKeyPath: path.join(fixture.directory, CANDIDATE_PUBLIC_KEY_NAME),
      expectedFingerprint: fixture.fingerprint,
      trustedFingerprint: fixture.fingerprint,
      trustedKeyId: fixture.keyId,
    });
    assert.equal(result.assets, primaryAssetNames().length);
    assert.equal(result.version, '0.0.1');
  } finally {
    await rm(fixture.directory, { recursive: true, force: true });
  }
});

test('bundle verifier rejects changed bytes, unexpected files, and replica drift', async () => {
  for (const mutation of ['archive', 'extra', 'replica']) {
    const fixture = await writeSignedFixture();
    try {
      if (mutation === 'archive') {
        await writeFile(path.join(fixture.directory, archiveName(TARGETS[0].target)), 'changed');
      } else if (mutation === 'extra') {
        await writeFile(path.join(fixture.directory, 'unexpected.txt'), 'unexpected');
      } else {
        const file = path.join(fixture.directory, 'REPRODUCIBILITY.json');
        const value = JSON.parse(await readFile(file, 'utf8'));
        value.targets[0].replicas[1].binarySha256 = 'd'.repeat(64);
        await writeFile(file, canonicalJson(value));
      }
      await assert.rejects(
        verifyReleaseBundle({
          directory: fixture.directory,
          publicKeyPath: path.join(fixture.directory, CANDIDATE_PUBLIC_KEY_NAME),
          expectedFingerprint: fixture.fingerprint,
          trustedFingerprint: fixture.fingerprint,
          trustedKeyId: fixture.keyId,
        }),
        /mismatch|reproducibility|exact set/iu,
      );
    } finally {
      await rm(fixture.directory, { recursive: true, force: true });
    }
  }
});

test('candidate assembler requires byte-reproducible binaries and archives per target', async () => {
  const temporary = await temporaryDirectory();
  const input = path.join(temporary, 'input');
  const output = path.join(temporary, 'output');
  const publicKeyPath = path.join(temporary, 'public.pem');
  const sourceArchivePath = path.join(temporary, SOURCE_ARCHIVE_NAME);
  const { publicKey } = generateKeyPairSync('ed25519');
  const commit = 'e'.repeat(40);
  try {
    await mkdir(input);
    await writeFile(publicKeyPath, publicKey.export({ type: 'spki', format: 'pem' }));
    await writeFile(sourceArchivePath, 'source archive fixture');
    for (const target of TARGETS) {
      const binaryFile = await writeTemporaryBytes(
        temporary,
        `binary-${target.target}`,
        target.target,
      );
      const binaryHash = await sha256File(binaryFile);
      for (const replica of [1, 2]) {
        const directory = path.join(input, `${target.target}-${replica}`);
        await mkdir(directory);
        const archive = archiveName(target.target);
        const archivePath = path.join(directory, archive);
        await writeFile(archivePath, `archive:${target.target}`);
        const archiveSha256 = await sha256File(archivePath);
        const archiveBytes = (await readFile(archivePath)).length;
        await writeFile(path.join(directory, `${archive}.sha256`), `${archiveSha256}  ${archive}`);
        await writeFile(
          path.join(directory, 'CANDIDATE-METADATA.json'),
          canonicalJson({
            schema: BUILD_METADATA_SCHEMA,
            version: '0.0.1',
            commit,
            target: target.target,
            support: target.support,
            replica,
            archive,
            archiveSha256,
            archiveBytes,
            binarySha256: binaryHash,
          }),
        );
      }
    }
    const result = spawnSync(
      process.execPath,
      [
        'scripts/assemble-release-candidate.mjs',
        '--input', input,
        '--output', output,
        '--source', root,
        '--public-key', publicKeyPath,
        '--source-archive', sourceArchivePath,
        '--version', '0.0.1',
        '--commit', commit,
      ],
      { cwd: root, encoding: 'utf8' },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /5 targets x 2 replicas/u);
    const reproducibility = JSON.parse(await readFile(path.join(output, 'REPRODUCIBILITY.json'), 'utf8'));
    assert.equal(reproducibility.targets.length, 5);
    assert.ok(reproducibility.targets.every((target) => target.replicas.length === 2));

    const changedTarget = TARGETS[0].target;
    const changedDirectory = path.join(input, `${changedTarget}-2`);
    const changedArchive = archiveName(changedTarget);
    const changedArchivePath = path.join(changedDirectory, changedArchive);
    await writeFile(changedArchivePath, 'checksum-consistent but different archive');
    const changedHash = await sha256File(changedArchivePath);
    await writeFile(path.join(changedDirectory, `${changedArchive}.sha256`), `${changedHash}  ${changedArchive}`);
    const changedMetadataPath = path.join(changedDirectory, 'CANDIDATE-METADATA.json');
    const changedMetadata = JSON.parse(await readFile(changedMetadataPath, 'utf8'));
    changedMetadata.archiveSha256 = changedHash;
    changedMetadata.archiveBytes = (await readFile(changedArchivePath)).length;
    await writeFile(changedMetadataPath, canonicalJson(changedMetadata));
    const rejected = spawnSync(
      process.execPath,
      [
        'scripts/assemble-release-candidate.mjs',
        '--input', input,
        '--output', path.join(temporary, 'rejected-output'),
        '--source', root,
        '--source-archive', sourceArchivePath,
        '--public-key', publicKeyPath,
        '--version', '0.0.1',
        '--commit', commit,
      ],
      { cwd: root, encoding: 'utf8' },
    );
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /archive is not byte-reproducible/u);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

async function writeTemporaryBytes(directory, name, bytes) {
  const file = path.join(directory, name.replaceAll(/[^A-Za-z0-9.-]/gu, '_'));
  await writeFile(file, bytes);
  return file;
}
