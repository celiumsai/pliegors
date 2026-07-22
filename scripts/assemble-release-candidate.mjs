#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { copyFile, lstat, mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import {
  BUILD_METADATA_SCHEMA,
  CANDIDATE_PUBLIC_KEY_NAME,
  REPRODUCIBILITY_SCHEMA,
  SOURCE_ARCHIVE_NAME,
  TARGETS,
  archiveName,
  assertRegularFile,
  canonicalJson,
  exactKeys,
  parseCliArgs,
  readCanonicalJson,
  requireCommit,
  requireExactNames,
  requireSemver,
  requireSha256,
  sha256File,
} from './release-bundle-lib.mjs';

const MAX_INPUT_FILES = 100;

async function findMetadataFiles(root) {
  const result = [];
  const queue = [root];
  let observed = 0;
  while (queue.length > 0) {
    const directory = queue.shift();
    const entries = await readdir(directory, { withFileTypes: true });
    for (const entry of entries) {
      observed += 1;
      if (observed > MAX_INPUT_FILES) throw new Error('candidate input exceeds file-entry bound');
      const child = path.join(directory, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`candidate input contains symlink: ${child}`);
      if (entry.isDirectory()) queue.push(child);
      else if (entry.isFile() && entry.name === 'CANDIDATE-METADATA.json') result.push(child);
      else if (!entry.isFile()) throw new Error(`candidate input contains unsupported entry: ${child}`);
    }
  }
  return result.sort((left, right) => left.localeCompare(right));
}

function validateMetadata(value, version, commit) {
  exactKeys(
    value,
    [
      'archive',
      'archiveBytes',
      'archiveSha256',
      'binarySha256',
      'commit',
      'replica',
      'schema',
      'support',
      'target',
      'version',
    ],
    'candidate metadata',
  );
  if (value.schema !== BUILD_METADATA_SCHEMA) throw new Error('unknown candidate metadata schema');
  if (requireSemver(value.version) !== version || requireCommit(value.commit) !== commit) {
    throw new Error('candidate metadata release identity mismatch');
  }
  const target = TARGETS.find((entry) => entry.target === value.target);
  if (!target || value.support !== target.support) throw new Error('candidate target/support mismatch');
  if (value.replica !== 1 && value.replica !== 2) throw new Error('candidate replica must be 1 or 2');
  if (value.archive !== archiveName(value.target)) throw new Error('candidate archive name mismatch');
  if (!Number.isSafeInteger(value.archiveBytes) || value.archiveBytes < 1) {
    throw new Error('invalid candidate archive byte size');
  }
  requireSha256(value.archiveSha256, 'candidate archive hash');
  requireSha256(value.binarySha256, 'candidate binary hash');
  return target;
}

async function loadBuild(metadataPath, version, commit) {
  const { value } = await readCanonicalJson(metadataPath, 'CANDIDATE-METADATA.json');
  const target = validateMetadata(value, version, commit);
  const directory = path.dirname(metadataPath);
  const archivePath = path.join(directory, value.archive);
  const sidecarPath = `${archivePath}.sha256`;
  const stat = await assertRegularFile(archivePath, value.archive);
  await assertRegularFile(sidecarPath, `${value.archive}.sha256`);
  if (stat.size !== value.archiveBytes || (await sha256File(archivePath)) !== value.archiveSha256) {
    throw new Error(`candidate archive bytes disagree with metadata: ${value.archive}`);
  }
  const sidecar = await readFile(sidecarPath, 'utf8');
  if (sidecar !== `${value.archiveSha256}  ${value.archive}`) {
    throw new Error(`candidate sidecar mismatch: ${value.archive}`);
  }
  return { ...value, archivePath, sidecarPath, expectedSupport: target.support };
}

async function assertEmptyOutput(output) {
  await mkdir(output, { recursive: true });
  const stat = await lstat(output);
  if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error('output must be a real directory');
  const entries = await readdir(output);
  if (entries.length !== 0) throw new Error('output directory must be empty');
}

async function main() {
  const args = parseCliArgs(process.argv.slice(2), [
    '--commit',
    '--input',
    '--output',
    '--public-key',
    '--source',
    '--source-archive',
    '--version',
  ]);
  for (const required of ['--commit', '--input', '--output', '--public-key', '--source', '--source-archive', '--version']) {
    if (!args[required]) throw new Error(`${required} is required`);
  }
  const version = requireSemver(args['--version']);
  const commit = requireCommit(args['--commit']);
  const input = path.resolve(args['--input']);
  const output = path.resolve(args['--output']);
  const source = path.resolve(args['--source']);
  const sourceArchive = path.resolve(args['--source-archive']);
  const publicKey = path.resolve(args['--public-key']);
  if (output === input || output.startsWith(`${input}${path.sep}`)) {
    throw new Error('output must be disjoint from candidate input');
  }
  await assertEmptyOutput(output);
  await assertRegularFile(publicKey, 'candidate public key');
  const sourceArchiveStat = await assertRegularFile(sourceArchive, SOURCE_ARCHIVE_NAME);
  if (sourceArchiveStat.size > 100 * 1024 * 1024) throw new Error('source archive exceeds 100 MiB');

  const metadataPaths = await findMetadataFiles(input);
  if (metadataPaths.length !== TARGETS.length * 2) {
    throw new Error(`expected ${TARGETS.length * 2} candidate metadata files`);
  }
  const builds = await Promise.all(metadataPaths.map((file) => loadBuild(file, version, commit)));
  const reproducibilityTargets = [];
  for (const target of TARGETS) {
    const replicas = builds
      .filter((build) => build.target === target.target)
      .sort((left, right) => left.replica - right.replica);
    if (replicas.length !== 2 || replicas[0].replica !== 1 || replicas[1].replica !== 2) {
      throw new Error(`target ${target.target} does not have exactly replicas 1 and 2`);
    }
    if (replicas[0].binarySha256 !== replicas[1].binarySha256) {
      throw new Error(`target ${target.target} is not binary reproducible`);
    }
    if (replicas[0].archiveSha256 !== replicas[1].archiveSha256) {
      throw new Error(`target ${target.target} archive is not byte-reproducible`);
    }
    const selected = replicas[0];
    await copyFile(selected.archivePath, path.join(output, selected.archive));
    await copyFile(selected.sidecarPath, path.join(output, `${selected.archive}.sha256`));
    reproducibilityTargets.push({
      target: target.target,
      support: target.support,
      binarySha256: selected.binarySha256,
      replicas: replicas.map((replica) => ({
        replica: replica.replica,
        binarySha256: replica.binarySha256,
        archiveSha256: replica.archiveSha256,
        archiveBytes: replica.archiveBytes,
      })),
    });
  }

  const sourceFiles = [
    ['scripts/install.ps1', 'install.ps1'],
    ['scripts/install.sh', 'install.sh'],
    ['scripts/run-golden-path.mjs', 'run-golden-path.mjs'],
    ['scripts/release-bundle-lib.mjs', 'release-bundle-lib.mjs'],
    ['scripts/source-archive-listing.mjs', 'source-archive-listing.mjs'],
    ['scripts/verify-release-bundle.mjs', 'verify-release-bundle.mjs'],
  ];
  for (const [relative, name] of sourceFiles) {
    const file = path.join(source, relative);
    await assertRegularFile(file, relative);
    await copyFile(file, path.join(output, name));
  }
  await copyFile(sourceArchive, path.join(output, SOURCE_ARCHIVE_NAME));
  await copyFile(publicKey, path.join(output, CANDIDATE_PUBLIC_KEY_NAME));
  await writeFile(
    path.join(output, 'REPRODUCIBILITY.json'),
    canonicalJson({ schema: REPRODUCIBILITY_SCHEMA, version, commit, targets: reproducibilityTargets }),
    { encoding: 'utf8', flag: 'wx' },
  );

  const names = await readdir(output);
  requireExactNames(
    names,
    [
      ...TARGETS.flatMap(({ target }) => [archiveName(target), `${archiveName(target)}.sha256`]),
      'install.ps1',
      'install.sh',
      'run-golden-path.mjs',
      'release-bundle-lib.mjs',
      'source-archive-listing.mjs',
      'REPRODUCIBILITY.json',
      SOURCE_ARCHIVE_NAME,
      'verify-release-bundle.mjs',
      CANDIDATE_PUBLIC_KEY_NAME,
    ],
    'assembled unsigned bundle',
  );
  console.log(`Candidate assembly PASS: ${TARGETS.length} targets x 2 replicas for ${commit}`);
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
