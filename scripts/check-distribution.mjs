// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash, createPublicKey } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const metadata = JSON.parse(execFileSync('cargo', [
  'metadata', '--no-deps', '--format-version', '1',
], { cwd: root, encoding: 'utf8' }));
const workspaceVersion = metadata.packages.find((pkg) => pkg.name === 'pliego-cli')?.version;
assert.ok(workspaceVersion, 'pliego-cli package is missing');

const crates = metadata.packages
  .filter((pkg) => pkg.manifest_path.replaceAll('\\', '/').includes('/crates/'))
  .sort((left, right) => left.name.localeCompare(right.name));
const expected = [
  'pliego-adapters', 'pliego-artifact', 'pliego-assets', 'pliego-cli', 'pliego-content', 'pliego-dom',
  'pliego-fold', 'pliego-hyphae', 'pliego-inspect', 'pliego-log', 'pliego-macros',
  'pliego-reactive', 'pliego-resume', 'pliego-ssg', 'pliego-starters',
].sort();
assert.deepEqual(crates.map((pkg) => pkg.name), expected);

for (const pkg of crates) {
  assert.equal(pkg.version, workspaceVersion, `${pkg.name} version drift`);
  assert.equal(pkg.license, 'Apache-2.0', `${pkg.name} license`);
  assert.equal(pkg.repository, 'https://github.com/celiumsai/pliegors', `${pkg.name} repository`);
  assert.equal(pkg.rust_version, '1.85', `${pkg.name} rust-version`);
  assert.ok(pkg.description?.trim(), `${pkg.name} description`);
  assert.deepEqual(pkg.publish, [], `${pkg.name} must reject registry publication`);
  for (const dependency of pkg.dependencies.filter((item) => item.name.startsWith('pliego-'))) {
    assert.ok(dependency.path, `${pkg.name} -> ${dependency.name} workspace path`);
    assert.equal(dependency.source, null, `${pkg.name} -> ${dependency.name} registry source`);
  }
}

const releasePath = path.join(root, '.github/workflows/release.yml');
const release = readFileSync(releasePath, 'utf8');
const ci = readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8');
const checkoutAction = 'actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0';
const setupNodeAction = 'actions/setup-node@820762786026740c76f36085b0efc47a31fe5020';
const uploadArtifactAction = 'actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a';
const downloadArtifactAction = 'actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c';
for (const [workflow, source] of [['ci.yml', ci], ['release.yml', release]]) {
  assert.ok(source.includes(checkoutAction), `${workflow} checkout action is not SHA-pinned`);
  assert.ok(source.includes(setupNodeAction), `${workflow} setup-node action is not SHA-pinned`);
  assert.ok(source.includes('persist-credentials: false'), `${workflow} persists checkout credentials`);
  assert.ok(!/actions\/(?:checkout|setup-node)@v\d/.test(source), `${workflow} uses a mutable action tag`);
}
for (const action of [uploadArtifactAction, downloadArtifactAction]) {
  assert.ok(release.includes(action), `release workflow lacks ${action}`);
}
for (const [, action, revision] of release.matchAll(/uses:\s+([^@\s]+)@([^\s#]+)/g)) {
  assert.match(revision, /^[0-9a-f]{40}$/, `${action} must use an immutable commit SHA`);
}
assert.ok(
  ci.includes('064948d58e2d6c0a745216477a639ba696216d6309aaa902939d1b865b1d869d'),
  'CI lacks the pinned wasm-bindgen-cli 0.2.126 digest',
);
assert.ok(
  ci.includes('8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8'),
  'CI lacks the pinned actionlint 1.7.12 Linux digest',
);
assert.ok(ci.includes('actionlint" .github/workflows/*.yml'), 'CI does not validate workflows');
assert.ok(!ci.includes('.sha256sum'), 'CI must not trust a checksum sidecar from the asset release');
const releaseTargets = [
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
  'x86_64-apple-darwin',
  'aarch64-apple-darwin',
  'x86_64-pc-windows-msvc',
].sort();
const matrixTargets = [...release.matchAll(/^\s+-\s+((?:aarch64|x86_64)-[^\s]+)$/gm)]
  .map((match) => match[1])
  .sort();
assert.deepEqual(matrixTargets, releaseTargets, 'release matrix must contain exactly five targets');

for (const contract of [
  'workflow_dispatch', "format('candidate:{0}', inputs.tag)", "format('draft:{0}', inputs.tag)",
  'PLIEGORS_SOURCE_REV', 'replica: [1, 2]',
  'ubuntu-24.04', 'ubuntu-24.04-arm', 'macos-15-intel', 'macos-15', 'windows-2025',
  'pliego-$env:RELEASE_TARGET.zip', 'retention-days: 7', 'retention-days: 14',
  'CANDIDATE-METADATA.json',
  'PLIEGORS_CANDIDATE_SIGNING_KEY', 'create-release-manifest.mjs',
  'verify-release-bundle.mjs', 'install.sh', 'install.ps1', 'golden_path',
  'gh release create', '--target "$GITHUB_SHA"', '--draft', '--latest=false',
]) assert.ok(release.includes(contract), `release candidate contract lacks ${contract}`);
assert.ok(release.includes("github.ref == 'refs/heads/main'"), 'draft mode must be restricted to main');
assert.equal(
  (release.match(/ref: \$\{\{ github\.sha \}\}/g) ?? []).length,
  2,
  'build and seal assembler must checkout the validated SHA',
);
assert.ok(release.includes('$expectedTag = "v$version"'), 'release tag must derive from Cargo version');
assert.ok(release.includes('$env:RELEASE_TAG -cne $expectedTag'), 'release tag must equal Cargo version');
assert.ok(release.includes("unknown-linux-gnu$') { 'production' } else { 'development' }"), 'support tier mapping drift');

assert.doesNotMatch(release, /^\s*push\s*:/m, 'release workflow must remain manual-only');
assert.doesNotMatch(release, /gh release (?:edit|upload)/, 'workflow must not mutate an existing release');
assert.doesNotMatch(release, /--(?:draft|latest)=true|--draft=false/, 'workflow may only create a draft');
assert.equal((release.match(/contents: write/g) ?? []).length, 1, 'only the draft job may write contents');
assert.ok(
  release.indexOf('contents: write') > release.indexOf('  draft:'),
  'contents write permission must be scoped to the draft assembler',
);
assert.ok(release.includes('contents: read'), 'default workflow permission must remain read-only');
assert.equal((release.match(/--latest=false/g) ?? []).length, 1, 'draft must opt out of mutable latest');
assert.ok(release.includes("grep -q 'HTTP 404'"), 'release existence checks must fail closed on API errors');
assert.doesNotMatch(
  release.replace('--latest=false', ''),
  /\blatest\b/i,
  'release workflow must not use mutable latest aliases',
);
assert.ok(release.includes('sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250'), 'release key fingerprint drift');
const createRelease = release.slice(release.indexOf('gh release create'));
assert.ok(createRelease.includes('release-assets/*'), 'draft release must upload the exact sealed bundle');
for (const forbidden of ['refs/tags/', 'crates.io', 'cloudflare', 'wrangler']) {
  assert.ok(!release.toLowerCase().includes(forbidden), `release workflow contains ${forbidden}`);
}

assert.equal(
  existsSync(path.join(root, '.github/workflows/publish-crates.yml')),
  false,
  'crates registry publication workflow must not exist',
);

const publicProjectFiles = [
  'CHANGELOG.md',
  'CODE_OF_CONDUCT.md',
  'CONTRIBUTING.md',
  'GOVERNANCE.md',
  'LICENSE',
  'NOTICE',
  'SECURITY.md',
  'SUPPORT.md',
  'THIRD_PARTY_NOTICES.md',
  'TRADEMARKS.md',
  '.github/PULL_REQUEST_TEMPLATE.md',
  '.github/dependabot.yml',
  '.github/ISSUE_TEMPLATE/bug_report.yml',
  '.github/ISSUE_TEMPLATE/feature_request.yml',
  '.github/ISSUE_TEMPLATE/config.yml',
  'brand/README.md',
  'brand/pliegors-app-icon.svg',
  'brand/pliegors-symbol.svg',
  'brand/pliegors-symbol-reversed.svg',
  'keys/pliegors-candidate-release.pub.pem',
  'scripts/assemble-release-candidate.mjs',
  'scripts/create-release-manifest.mjs',
  'scripts/release-bundle-lib.mjs',
  'scripts/verify-release-bundle.mjs',
  'crates/pliego-starters/LICENSE',
  'workers/pliegors-email/package-lock.json',
  'workers/pliegors-email/README.md',
  'workers/pliegors-email/src/handler.ts',
  'workers/pliegors-email/src/index.ts',
  'workers/pliegors-email/wrangler.jsonc',
];
for (const file of publicProjectFiles) {
  const absolute = path.join(root, file);
  assert.ok(existsSync(absolute), `public project contract lacks ${file}`);
  assert.ok(readFileSync(absolute).length > 0, `public project file is empty: ${file}`);
}

const candidateKeyPath = path.join(root, 'keys/pliegors-candidate-release.pub.pem');
assert.deepEqual(
  readdirSync(path.join(root, 'keys')).sort(),
  ['pliegors-candidate-release.pub.pem'],
  'keys directory must contain only the candidate public key',
);
const candidateKey = createPublicKey(readFileSync(candidateKeyPath));
const candidateFingerprint = `sha256:${createHash('sha256')
  .update(candidateKey.export({ type: 'spki', format: 'der' }))
  .digest('hex')}`;
assert.equal(
  candidateFingerprint,
  'sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250',
  'candidate public key fingerprint drift',
);
const releaseBundleSources = [
  'scripts/assemble-release-candidate.mjs',
  'scripts/create-release-manifest.mjs',
  'scripts/release-bundle-lib.mjs',
  'scripts/verify-release-bundle.mjs',
].map((file) => readFileSync(path.join(root, file), 'utf8'));
for (const token of [
  'dev.pliegors.candidate-build/v1',
  'dev.pliegors.release-manifest/v1',
  'dev.pliegors.release-reproducibility/v1',
  'Ed25519',
  'replicasPerTarget',
  'expected-key-fingerprint',
]) {
  assert.ok(
    releaseBundleSources.some((source) => source.includes(token)),
    `release bundle sources lack ${token}`,
  );
}

const brandIcon = readFileSync(path.join(root, 'brand/pliegors-app-icon.svg'), 'utf8');
for (const token of ['#171916', '#f3f4ee', '#c23a30', 'PliegoRS application icon']) {
  assert.ok(brandIcon.includes(token), `canonical app icon lacks ${token}`);
}
const emailWorkerConfig = readFileSync(
  path.join(root, 'workers/pliegors-email/wrangler.jsonc'),
  'utf8',
);
const emailWorkerSource = readFileSync(
  path.join(root, 'workers/pliegors-email/src/handler.ts'),
  'utf8',
);
assert.ok(emailWorkerConfig.includes('"compatibility_date": "2026-07-14"'), 'email Worker compatibility date drift');
assert.ok(emailWorkerConfig.includes('"nodejs_compat"'), 'email Worker lacks nodejs_compat');
assert.ok(!emailWorkerConfig.includes('FORWARD_TO'), 'email Worker secret appears in Wrangler config');
assert.ok(emailWorkerSource.includes('const PUBLIC_RECIPIENT = "hello@pliegors.dev"'), 'email Worker recipient drift');
assert.ok(emailWorkerSource.includes('env.FORWARD_TO'), 'email Worker lacks secret destination binding');
assert.ok(emailWorkerSource.indexOf('message.forward') < emailWorkerSource.indexOf('message.reply'), 'email Worker must forward before replying');

const installerSources = {
  shell: readFileSync(path.join(root, 'scripts/install.sh'), 'utf8'),
  powershell: readFileSync(path.join(root, 'scripts/install.ps1'), 'utf8'),
};
const githubReleaseBase = 'https://github.com/celiumsai/pliegors/releases/download';
for (const [name, source] of Object.entries(installerSources)) {
  assert.ok(source.includes(githubReleaseBase), `${name} installer lacks the GitHub release base`);
  assert.match(source, /release selector is required/iu, `${name} installer must require a release selector`);
  assert.match(source, /channel latest/iu, `${name} installer must expose the explicit latest channel`);
  assert.ok(source.includes('.sha256'), `${name} installer must fetch a checksum sidecar`);
  assert.match(source, /sha256 mismatch/iu, `${name} installer must fail on checksum mismatch`);
  assert.doesNotMatch(source, /api\.github\.com|latest\.txt|dl\.pliego\.run/iu);
}
assert.ok(
  installerSources.shell.includes('archive="pliego-$target.zip"'),
  'Unix installer archive name must be stable per target',
);
assert.ok(
  installerSources.powershell.includes('$archive = "pliego-$target.zip"'),
  'Windows installer archive name must be stable per target',
);
assert.ok(!installerSources.shell.includes('archive="pliego-$version-'), 'Unix archive name contains a mutable version');
assert.ok(!installerSources.powershell.includes('$archive = "pliego-$Version-'), 'Windows archive name contains a mutable version');
for (const target of [
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
  'x86_64-apple-darwin',
  'aarch64-apple-darwin',
]) {
  assert.ok(installerSources.shell.includes(target), `Unix installer lacks ${target}`);
}
assert.ok(
  installerSources.powershell.includes('x86_64-pc-windows-msvc'),
  'Windows installer lacks x86_64-pc-windows-msvc',
);

const cli = readFileSync(path.join(root, 'crates/pliego-cli/src/main.rs'), 'utf8');
const cliBuild = readFileSync(path.join(root, 'crates/pliego-cli/build.rs'), 'utf8');
assert.ok(cli.includes('https://github.com/celiumsai/pliegors'), 'starter source repository');
assert.ok(cli.includes('PLIEGORS_SOURCE_REV'), 'starter build revision override');
assert.match(cli, /rev = \\\"\{\}\\\"/, 'starter dependency must use an exact Git revision');
assert.ok(!cli.includes('registry version'), 'starter dependency must not use a registry');
assert.ok(!cli.includes('FALLBACK_PLIEGORS_SOURCE_REV'), 'starter revision must not be hard-coded');
assert.ok(cliBuild.includes('rev-parse'), 'CLI build must resolve the source checkout revision');
assert.ok(cliBuild.includes('HEAD^{commit}'), 'CLI build must verify the source revision as a commit');
assert.ok(cliBuild.includes('cargo:rerun-if-env-changed'), 'CLI build must track the release override');

console.log(
  `Distribution contract PASS: ${crates.length} source-only crates @ ${workspaceVersion}, ` +
  '5 targets x 2 replicas, signed private candidate, and gated manual draft',
);
