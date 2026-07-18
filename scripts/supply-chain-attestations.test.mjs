// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '..');
const createScript = path.join(root, 'scripts/create-supply-chain-attestations.mjs');
const verifyScript = path.join(root, 'scripts/verify-supply-chain-attestations.mjs');
const commit = '0123456789abcdef0123456789abcdef01234567';

function run(script, args) {
  return spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });
}

async function fixture() {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'pliegors-attestations-'));
  const release = path.join(directory, 'release');
  const output = path.join(directory, 'attestations');
  await mkdir(release);
  await writeFile(path.join(release, 'pliego-x86_64-unknown-linux-gnu.zip'), 'archive-bytes');
  await writeFile(path.join(release, 'REPRODUCIBILITY.json'), '{}\n');
  await writeFile(path.join(release, 'RELEASE-MANIFEST.json.sig'), 'signature\n');
  await writeFile(path.join(release, 'RELEASE-MANIFEST.json'), `${JSON.stringify({
    schema: 'dev.pliegors.release-manifest/v1',
    release: { version: '0.0.2', tag: 'v0.0.2', commit, sourceDateEpoch: 1 },
    assets: [{ name: 'pliego-x86_64-unknown-linux-gnu.zip', role: 'cli-archive', bytes: 13, sha256: '0'.repeat(64) }],
  }, null, 2)}\n`);
  const sbom = path.join(directory, 'bom.json');
  await writeFile(sbom, `${JSON.stringify({
    bomFormat: 'CycloneDX', specVersion: '1.6', version: 1,
    metadata: { component: { type: 'application', name: 'pliego-cli', version: '0.0.2' } },
    components: [{ type: 'library', name: 'serde', version: '1.0.0' }],
  })}\n`);
  return { directory, release, output, sbom };
}

test('attestation package binds the sealed release, SBOM, source, and workflow identity', async () => {
  const value = await fixture();
  try {
    const created = run(createScript, [
      '--release', value.release, '--output', value.output, '--sbom', value.sbom, '--source', root,
      '--version', '0.0.2', '--commit', commit, '--run-id', '12345',
      '--workflow-ref', 'https://github.com/celiumsai/pliegors/.github/workflows/release.yml@refs/heads/main',
    ]);
    assert.equal(created.status, 0, created.stderr);
    await writeFile(path.join(value.output, 'ATTESTATIONS.sigstore.json'), '{}\n');
    const verified = run(verifyScript, ['--release', value.release, '--attestations', value.output]);
    assert.equal(verified.status, 0, verified.stderr);
    const provenance = JSON.parse(await readFile(path.join(value.output, 'PLIEGORS.intoto.jsonl'), 'utf8'));
    assert.equal(provenance.predicateType, 'https://slsa.dev/provenance/v1');
    assert.equal(provenance.predicate.buildDefinition.resolvedDependencies[0].digest.gitCommit, commit);

    await writeFile(path.join(value.release, 'pliego-x86_64-unknown-linux-gnu.zip'), 'tampered');
    const rejected = run(verifyScript, ['--release', value.release, '--attestations', value.output]);
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /provenance subject mismatch/u);
  } finally {
    await rm(value.directory, { recursive: true, force: true });
  }
});

test('attestation generator rejects a foreign release identity and output reuse', async () => {
  const value = await fixture();
  try {
    const foreign = run(createScript, [
      '--release', value.release, '--output', value.output, '--sbom', value.sbom, '--source', root,
      '--version', '0.0.3', '--commit', commit, '--run-id', '12345',
      '--workflow-ref', 'https://github.com/celiumsai/pliegors/.github/workflows/release.yml@refs/heads/main',
    ]);
    assert.notEqual(foreign.status, 0);
    assert.match(foreign.stderr, /identity mismatch/u);
  } finally {
    await rm(value.directory, { recursive: true, force: true });
  }
});
