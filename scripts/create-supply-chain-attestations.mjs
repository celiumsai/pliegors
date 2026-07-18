#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { copyFile, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import {
  ATTESTATION_MANIFEST,
  ATTESTATION_SCHEMA,
  PROVENANCE_NAME,
  PROVENANCE_PREDICATE,
  PROVENANCE_TYPE,
  RELEASE_BUILD_TYPE,
  SBOM_NAME,
  assertRegularFile,
  canonicalJson,
  jsonLine,
  parseArgs,
  readJson,
  regularFileNames,
  requireCommit,
  requireSemver,
  sha256File,
} from './supply-chain-attestations-lib.mjs';

async function main() {
  const args = parseArgs(process.argv.slice(2), [
    '--commit', '--output', '--release', '--run-id', '--sbom', '--source', '--version', '--workflow-ref',
  ]);
  for (const name of ['--commit', '--output', '--release', '--run-id', '--sbom', '--source', '--version', '--workflow-ref']) {
    if (!args[name]) throw new Error(`${name} is required`);
  }
  const version = requireSemver(args['--version']);
  const commit = requireCommit(args['--commit']);
  const runId = args['--run-id'];
  if (!/^[1-9][0-9]{0,19}$/u.test(runId)) throw new Error('invalid GitHub run ID');
  const workflowRef = args['--workflow-ref'];
  if (!/^https:\/\/github\.com\/celiumsai\/pliegors\/\.github\/workflows\/release\.yml@refs\/(?:heads|tags)\/[A-Za-z0-9._\/-]+$/u.test(workflowRef)) {
    throw new Error('invalid release workflow identity');
  }
  const release = path.resolve(args['--release']);
  const output = path.resolve(args['--output']);
  const sbomPath = path.resolve(args['--sbom']);
  const source = path.resolve(args['--source']);
  if (output === release || output.startsWith(`${release}${path.sep}`)) {
    throw new Error('attestation output must be disjoint from the sealed release');
  }
  await mkdir(output, { recursive: true });
  if ((await regularFileNames(output)).length !== 0) throw new Error('attestation output must be empty');

  const releaseNames = await regularFileNames(release);
  if (!releaseNames.includes('RELEASE-MANIFEST.json') || !releaseNames.includes('RELEASE-MANIFEST.json.sig')) {
    throw new Error('sealed release manifest and signature are required');
  }
  const releaseManifest = await readJson(path.join(release, 'RELEASE-MANIFEST.json'));
  const releaseIdentity = releaseManifest.value?.release;
  if (releaseIdentity?.version !== version || releaseIdentity?.commit !== commit || releaseIdentity?.tag !== `v${version}`) {
    throw new Error('sealed release identity mismatch');
  }

  const { value: sbom } = await readJson(sbomPath, 'CycloneDX SBOM');
  if (sbom?.bomFormat !== 'CycloneDX' || typeof sbom.specVersion !== 'string' || !Array.isArray(sbom.components)) {
    throw new Error('SBOM is not a supported CycloneDX document');
  }
  const normalizedSbom = Buffer.from(canonicalJson(sbom), 'utf8');
  const normalizedSbomPath = path.join(output, SBOM_NAME);
  await writeFile(normalizedSbomPath, normalizedSbom, { flag: 'wx' });

  const subjects = [];
  for (const name of releaseNames) {
    const file = path.join(release, name);
    await assertRegularFile(file, name);
    subjects.push({ name: `release/${name}`, digest: { sha256: await sha256File(file) } });
  }
  subjects.push({ name: `attestations/${SBOM_NAME}`, digest: { sha256: await sha256File(normalizedSbomPath) } });
  subjects.sort((left, right) => left.name.localeCompare(right.name));
  const provenance = {
    _type: PROVENANCE_TYPE,
    subject: subjects,
    predicateType: PROVENANCE_PREDICATE,
    predicate: {
      buildDefinition: {
        buildType: RELEASE_BUILD_TYPE,
        externalParameters: { version, targets: releaseManifest.value.assets.filter((asset) => asset.role === 'cli-archive').map((asset) => asset.name).sort() },
        internalParameters: { releaseManifestSha256: await sha256File(path.join(release, 'RELEASE-MANIFEST.json')) },
        resolvedDependencies: [{ uri: `git+https://github.com/celiumsai/pliegors@${commit}`, digest: { gitCommit: commit } }],
      },
      runDetails: {
        builder: { id: workflowRef },
        metadata: { invocationId: `https://github.com/celiumsai/pliegors/actions/runs/${runId}` },
        byproducts: [{ name: 'two-replica-reproducibility', digest: { sha256: await sha256File(path.join(release, 'REPRODUCIBILITY.json')) } }],
      },
    },
  };
  const provenancePath = path.join(output, PROVENANCE_NAME);
  await writeFile(provenancePath, jsonLine(provenance), { flag: 'wx' });

  const verifierFiles = ['supply-chain-attestations-lib.mjs', 'verify-supply-chain-attestations.mjs'];
  for (const name of verifierFiles) {
    const input = path.join(source, 'scripts', name);
    await assertRegularFile(input, name);
    await copyFile(input, path.join(output, name));
  }

  const fileNames = [SBOM_NAME, PROVENANCE_NAME, ...verifierFiles].sort();
  const manifest = {
    schema: ATTESTATION_SCHEMA,
    release: { version, tag: `v${version}`, commit },
    releaseManifest: { name: 'RELEASE-MANIFEST.json', sha256: await sha256File(path.join(release, 'RELEASE-MANIFEST.json')) },
    files: fileNames.map((name) => ({ name, sha256: null })),
    signing: { system: 'sigstore', artifact: 'ATTESTATIONS.json', bundle: 'ATTESTATIONS.sigstore.json' },
  };
  for (const entry of manifest.files) entry.sha256 = await sha256File(path.join(output, entry.name));
  await writeFile(path.join(output, ATTESTATION_MANIFEST), canonicalJson(manifest), { flag: 'wx' });
  console.log(`Supply-chain attestations PASS: ${subjects.length} subjects for v${version}`);
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
