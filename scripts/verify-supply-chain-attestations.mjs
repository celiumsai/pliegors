#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import path from 'node:path';
import {
  ATTESTATION_MANIFEST,
  ATTESTATION_SIGNATURE,
  ATTESTATION_SCHEMA,
  PROVENANCE_NAME,
  PROVENANCE_PREDICATE,
  PROVENANCE_TYPE,
  RELEASE_BUILD_TYPE,
  SBOM_NAME,
  assertRegularFile,
  exactKeys,
  parseArgs,
  readJson,
  regularFileNames,
  requireCommit,
  requireSemver,
  requireSha256,
  sha256File,
} from './supply-chain-attestations-lib.mjs';

async function main() {
  const args = parseArgs(process.argv.slice(2), ['--attestations', '--release']);
  if (!args['--attestations'] || !args['--release']) throw new Error('--attestations and --release are required');
  const directory = path.resolve(args['--attestations']);
  const release = path.resolve(args['--release']);
  const names = await regularFileNames(directory);
  const verifierFiles = ['supply-chain-attestations-lib.mjs', 'verify-supply-chain-attestations.mjs'];
  const unsignedNames = [ATTESTATION_MANIFEST, SBOM_NAME, PROVENANCE_NAME, ...verifierFiles].sort();
  const signedNames = [...unsignedNames, ATTESTATION_SIGNATURE].sort();
  if (names.join('\n') !== signedNames.join('\n')) {
    throw new Error('attestation package exact set mismatch');
  }
  await assertRegularFile(path.join(directory, ATTESTATION_SIGNATURE), ATTESTATION_SIGNATURE);
  for (const required of [ATTESTATION_MANIFEST, SBOM_NAME, PROVENANCE_NAME]) {
    if (!names.includes(required)) throw new Error(`missing attestation file: ${required}`);
  }
  const { value: manifest } = await readJson(path.join(directory, ATTESTATION_MANIFEST));
  exactKeys(manifest, ['files', 'release', 'releaseManifest', 'schema', 'signing'], 'attestation manifest');
  if (manifest.schema !== ATTESTATION_SCHEMA) throw new Error('unknown attestation schema');
  exactKeys(manifest.signing, ['artifact', 'bundle', 'system'], 'attestation signing');
  if (manifest.signing.system !== 'sigstore' || manifest.signing.artifact !== ATTESTATION_MANIFEST || manifest.signing.bundle !== ATTESTATION_SIGNATURE) {
    throw new Error('attestation signing contract mismatch');
  }
  const version = requireSemver(manifest.release?.version);
  const commit = requireCommit(manifest.release?.commit);
  if (manifest.release.tag !== `v${version}`) throw new Error('attestation tag mismatch');
  if (manifest.releaseManifest?.name !== 'RELEASE-MANIFEST.json') throw new Error('release manifest name mismatch');
  requireSha256(manifest.releaseManifest.sha256, 'release manifest digest');
  if (await sha256File(path.join(release, 'RELEASE-MANIFEST.json')) !== manifest.releaseManifest.sha256) {
    throw new Error('attestation package targets another release manifest');
  }
  const expectedFiles = [SBOM_NAME, PROVENANCE_NAME, ...verifierFiles];
  if (!Array.isArray(manifest.files) || manifest.files.map((entry) => entry.name).sort().join('\n') !== expectedFiles.sort().join('\n')) {
    throw new Error('attestation manifest file set mismatch');
  }
  for (const entry of manifest.files) {
    requireSha256(entry.sha256, `${entry.name} digest`);
    if (await sha256File(path.join(directory, entry.name)) !== entry.sha256) throw new Error(`${entry.name} digest mismatch`);
  }
  const { value: sbom } = await readJson(path.join(directory, SBOM_NAME));
  if (sbom?.bomFormat !== 'CycloneDX' || !Array.isArray(sbom.components)) throw new Error('invalid CycloneDX SBOM');
  const { value: provenance } = await readJson(path.join(directory, PROVENANCE_NAME));
  if (provenance?._type !== PROVENANCE_TYPE || provenance?.predicateType !== PROVENANCE_PREDICATE || provenance?.predicate?.buildDefinition?.buildType !== RELEASE_BUILD_TYPE) {
    throw new Error('invalid SLSA provenance envelope');
  }
  const dependency = provenance.predicate.buildDefinition.resolvedDependencies?.[0];
  if (dependency?.digest?.gitCommit !== commit || !dependency.uri?.endsWith(`@${commit}`)) {
    throw new Error('provenance source revision mismatch');
  }
  const releaseNames = await regularFileNames(release);
  const subjects = new Map(provenance.subject?.map((subject) => [subject.name, subject.digest?.sha256]));
  for (const name of releaseNames) {
    const digest = subjects.get(`release/${name}`);
    if (digest !== await sha256File(path.join(release, name))) throw new Error(`provenance subject mismatch: ${name}`);
  }
  if (subjects.size !== releaseNames.length + 1 || subjects.get(`attestations/${SBOM_NAME}`) !== await sha256File(path.join(directory, SBOM_NAME))) {
    throw new Error('provenance subject exact set mismatch');
  }
  console.log(`Supply-chain verification PASS: ${subjects.size} subjects for v${version} (${commit})`);
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
