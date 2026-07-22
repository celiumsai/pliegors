// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash, createPublicKey, verify } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const metadata = JSON.parse(execFileSync('cargo', [
  'metadata', '--no-deps', '--format-version', '1',
], { cwd: root, encoding: 'utf8' }));
const workspaceVersion = metadata.packages.find((pkg) => pkg.name === 'pliego-cli')?.version;
assert.ok(workspaceVersion, 'pliego-cli package is missing');

const product = JSON.parse(readFileSync(path.join(root, 'product.capabilities.json'), 'utf8'));
const unreleasedCrates = new Set(product.framework.unreleasedCrates);
const allCrates = metadata.packages
  .filter((pkg) => pkg.manifest_path.replaceAll('\\', '/').includes('/crates/'))
  .sort((left, right) => left.name.localeCompare(right.name));
for (const name of unreleasedCrates) {
  assert.ok(allCrates.some((pkg) => pkg.name === name), `unknown unreleased crate: ${name}`);
}
const crates = allCrates.filter((pkg) => !unreleasedCrates.has(pkg.name));
const expected = [
  'pliego-adapters', 'pliego-artifact', 'pliego-assets', 'pliego-cli', 'pliego-content', 'pliego-data', 'pliego-dom',
  'pliego-fold', 'pliego-hyphae', 'pliego-inspect', 'pliego-log', 'pliego-macros',
  'pliego-reactive', 'pliego-resume', 'pliego-router', 'pliego-runtime', 'pliego-sdk', 'pliego-ssg',
  'pliego-starters',
].sort();
assert.deepEqual(crates.map((pkg) => pkg.name), expected);
const packagesByName = new Map(allCrates.map((pkg) => [pkg.name, pkg]));
for (const pkg of allCrates) {
  assert.equal(pkg.version, workspaceVersion, `${pkg.name} version drift`);
  assert.equal(pkg.license, 'Apache-2.0', `${pkg.name} license`);
  assert.equal(pkg.repository, 'https://github.com/celiumsai/pliegors', `${pkg.name} repository`);
  assert.equal(pkg.homepage, 'https://pliegors.dev', `${pkg.name} homepage`);
  assert.equal(pkg.rust_version, '1.86', `${pkg.name} rust-version`);
  assert.ok(pkg.description?.trim(), `${pkg.name} description`);
  assert.deepEqual(pkg.publish, ['crates-io'], `${pkg.name} registry allowlist`);
  const readme = pkg.readme && path.resolve(path.dirname(pkg.manifest_path), pkg.readme);
  assert.ok(readme && path.basename(readme) === 'README.md' && existsSync(readme), `${pkg.name} readme`);
  for (const dependency of pkg.dependencies.filter((item) => item.name.startsWith('pliego-'))) {
    const dependencyPackage = packagesByName.get(dependency.name);
    assert.ok(dependencyPackage, `${pkg.name} -> ${dependency.name} workspace package`);
    assert.ok(dependency.path, `${pkg.name} -> ${dependency.name} workspace path`);
    assert.equal(dependency.source, null, `${pkg.name} -> ${dependency.name} registry source`);
    assert.equal(
      dependency.req,
      `=${dependencyPackage.version}`,
      `${pkg.name} -> ${dependency.name} version`,
    );
  }
}

const releasePath = path.join(root, '.github/workflows/release.yml');
const release = readFileSync(releasePath, 'utf8');
const ci = readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8');
const codeql = readFileSync(path.join(root, '.github/workflows/codeql.yml'), 'utf8');
const firstPartyNodePackages = [
  'package.json',
  'workers/pliegors-site/package.json',
  'workers/pliegors-email/package.json',
];
for (const relativePath of firstPartyNodePackages) {
  const manifest = JSON.parse(readFileSync(path.join(root, relativePath), 'utf8'));
  assert.equal(manifest.private, true, `${relativePath} must remain private to block npm publication`);
  assert.ok(
    !Object.values(manifest.scripts ?? {}).some((script) => /(?:^|\s)(?:npm|pnpm|yarn)\s+publish(?:\s|$)/u.test(script)),
    `${relativePath} must not expose a package-registry publish script`,
  );
}
const checkoutAction = 'actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0';
const setupNodeAction = 'actions/setup-node@820762786026740c76f36085b0efc47a31fe5020';
const uploadArtifactAction = 'actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a';
const downloadArtifactAction = 'actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c';
for (const [workflow, source] of [['ci.yml', ci], ['release.yml', release], ['codeql.yml', codeql]]) {
  assert.ok(source.includes(checkoutAction), `${workflow} checkout action is not SHA-pinned`);
  assert.ok(source.includes('persist-credentials: false'), `${workflow} persists checkout credentials`);
  assert.ok(!/actions\/checkout@v\d/.test(source), `${workflow} uses a mutable checkout tag`);
}
for (const [workflow, source] of [['ci.yml', ci], ['release.yml', release], ['codeql.yml', codeql]]) {
  assert.doesNotMatch(source, /(?:npm|pnpm|yarn)\s+publish(?:\s|$)/u, `${workflow} must not publish first-party Node packages`);
}
for (const [workflow, source] of [['ci.yml', ci], ['release.yml', release]]) {
  assert.ok(source.includes(setupNodeAction), `${workflow} setup-node action is not SHA-pinned`);
  assert.ok(!/actions\/setup-node@v\d/.test(source), `${workflow} uses a mutable setup-node tag`);
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
  'workflow_dispatch', 'channel:', 'canary', 'beta', 'stable',
  "inputs.channel == 'canary'", "inputs.channel != 'canary'",
  "format('candidate:{0}', inputs.tag)", "format('draft:{0}', inputs.tag)",
  'replica: [1, 2]',
  'ubuntu-22.04', 'ubuntu-24.04', 'ubuntu-24.04-arm', 'macos-15-intel', 'macos-15', 'windows-2025',
  'link-arg=/Brepro',
  'pliego-$env:RELEASE_TARGET.zip', 'retention-days: 7', 'retention-days: 14',
  'CANDIDATE-METADATA.json',
  'PLIEGORS_CANDIDATE_SIGNING_KEY', 'create-release-manifest.mjs',
  'verify-release-bundle.mjs', 'install.sh', 'install.ps1', 'golden_path',
  'golden_evidence:', 'run-golden-path.mjs', 'pliegors-source.tar.gz',
  'container-linux-x64', 'windows-unicode', 'windows-long-path',
  'P8-GOLDEN-MATRIX.json', 'wsl_report_base64',
  'P8-GOLDEN-MATRIX.sigstore.json',
  'create-deterministic-zip.mjs',
  'git -C source archive', 'gzip -n -9',
  'cmp pliegors-source-1.tar.gz pliegors-source-2.tar.gz',
  '--source-archive pliegors-source.tar.gz',
  'supply_chain:', 'cargo-cyclonedx --version 0.5.9 --locked',
  'create-supply-chain-attestations.mjs', 'verify-supply-chain-attestations.mjs',
  'ATTESTATIONS.sigstore.json', 'cosign sign-blob', 'cosign verify-blob',
  'https://token.actions.githubusercontent.com', 'id-token: write',
  'PLIEGORS_INSTALLER_ALLOW_UNSEALED',
  'gh release create', '--target "$GITHUB_SHA"', '--draft', '--prerelease', '--latest=false',
]) assert.ok(release.includes(contract), `release candidate contract lacks ${contract}`);
assert.equal((release.match(/PLIEGORS_INSTALLER_ALLOW_UNSEALED/g) ?? []).length, 2, 'unsealed installer bypass must exist only in replica smoke steps');
assert.doesNotMatch(release, /PliegoRS v0\.0\.1|--version 0\.0\.1/u, 'release notes must use the requested exact version');
assert.ok(release.includes("github.ref == 'refs/heads/main'"), 'draft mode must be restricted to main');
assert.equal(
  (release.match(/ref: \$\{\{ github\.sha \}\}/g) ?? []).length,
  4,
  'build, seal, attestation, and golden evidence jobs must checkout the validated SHA',
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
assert.ok(release.includes('node scripts/create-deterministic-zip.mjs'), 'release archives must use the deterministic ZIP writer');
assert.doesNotMatch(release, /Compress-Archive/, 'release workflow must not reintroduce timestamp-dependent PowerShell ZIPs');
const createRelease = release.slice(release.indexOf('gh release create'));
assert.ok(createRelease.includes('release-assets/*'), 'draft release must upload the exact sealed bundle');
assert.ok(createRelease.includes('attestations/*'), 'draft release must upload verified supply-chain attestations');
assert.ok(createRelease.includes('golden-evidence/P8-GOLDEN-MATRIX.json'), 'draft release must upload golden matrix evidence');
assert.ok(createRelease.includes('golden-evidence/P8-GOLDEN-MATRIX.sigstore.json'), 'draft release must upload signed golden identity');
for (const forbidden of ['refs/tags/', 'cargo publish', 'cloudflare', 'wrangler']) {
  assert.ok(!release.toLowerCase().includes(forbidden), `release workflow contains ${forbidden}`);
}

assert.equal(
  existsSync(path.join(root, '.github/workflows/publish-crates.yml')),
  false,
  'first crates.io publication must remain a guarded local operation',
);

const cratePublisher = readFileSync(path.join(root, 'scripts/publish-crates.mjs'), 'utf8');
for (const token of [
  'CARGO_REGISTRY_TOKEN', 'PLIEGORS_PUBLISH_CONFIRMATION', 'publish:v${version}',
  'origin/main', 'publish', '--locked', 'https://crates.io/api/v1/crates/',
]) assert.ok(cratePublisher.includes(token), `crate publisher lacks ${token}`);
assert.ok(!cratePublisher.includes('--token'), 'crate publisher must not expose the token on the command line');

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
  '.github/workflows/codeql.yml',
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
  'scripts/create-deterministic-zip.mjs',
  'scripts/create-supply-chain-attestations.mjs',
  'scripts/release-bundle-lib.mjs',
  'scripts/supply-chain-attestations-lib.mjs',
  'scripts/verify-supply-chain-attestations.mjs',
  'scripts/verify-release-bundle.mjs',
  'scripts/run-golden-path.mjs',
  'scripts/source-archive-listing.mjs',
  'scripts/check-golden-matrix.mjs',
  'scripts/check-telemetry-contract.mjs',
  'schemas/pliego.golden-path-report.schema.json',
  'schemas/pliego.golden-matrix.schema.json',
  'schemas/pliego.telemetry-report.schema.json',
  'docs/41-voluntary-telemetry.md',
  'scripts/publish-crates.mjs',
  'crates/pliego-starters/LICENSE',
  'examples/pliegors-site/public/fonts/LICENSE-fragment-mono.txt',
  'examples/pliegors-site/public/fonts/LICENSE-instrument-sans.txt',
  'examples/pliegors-site/public/fonts/LICENSE-instrument-serif.txt',
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
assert.ok(codeql.includes('github/codeql-action/init@7188fc363630916deb702c7fdcf4e481b751f97a'), 'CodeQL init action is not SHA-pinned');
assert.ok(codeql.includes('github/codeql-action/analyze@7188fc363630916deb702c7fdcf4e481b751f97a'), 'CodeQL analyze action is not SHA-pinned');
for (const language of ['rust', 'javascript-typescript', 'actions']) {
  assert.ok(codeql.includes(`- ${language}`), `CodeQL lacks ${language}`);
}
assert.ok(codeql.includes("repository.visibility == 'public'"), 'CodeQL must remain inert before public visibility');

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

const r6Directory = path.join(root, 'docs/evidence/r6');
const r6ManifestBytes = readFileSync(path.join(r6Directory, 'RELEASE-MANIFEST.json'));
const r6ManifestText = r6ManifestBytes.toString('utf8');
const r6Manifest = JSON.parse(r6ManifestText);
assert.equal(
  r6ManifestText,
  `${JSON.stringify(r6Manifest, null, 2)}\n`,
  'committed R6 manifest must remain canonical JSON',
);
assert.equal(r6Manifest.schema, 'dev.pliegors.release-manifest/v1');
assert.equal(r6Manifest.signing.algorithm, 'Ed25519');
assert.equal(r6Manifest.signing.publicKeySha256, candidateFingerprint);
const r6SignatureText = readFileSync(
  path.join(r6Directory, 'RELEASE-MANIFEST.json.sig'),
  'utf8',
);
assert.match(r6SignatureText, /^[A-Za-z0-9+/]{86}==\n$/u, 'R6 signature encoding');
assert.ok(
  verify(null, r6ManifestBytes, candidateKey, Buffer.from(r6SignatureText.trim(), 'base64')),
  'committed R6 manifest signature is invalid',
);
const r6ReproducibilityBytes = readFileSync(path.join(r6Directory, 'REPRODUCIBILITY.json'));
const r6Reproducibility = JSON.parse(r6ReproducibilityBytes.toString('utf8'));
assert.equal(r6Reproducibility.schema, 'dev.pliegors.release-reproducibility/v1');
assert.equal(r6Reproducibility.commit, r6Manifest.release.commit);
assert.equal(r6Reproducibility.version, r6Manifest.release.version);
assert.deepEqual(
  r6Reproducibility.targets.map((target) => target.target),
  releaseTargets,
  'R6 evidence target set drift',
);
for (const target of r6Reproducibility.targets) {
  assert.equal(target.replicas.length, 2, `${target.target} replica count`);
  assert.deepEqual(target.replicas.map((replica) => replica.replica), [1, 2]);
  assert.ok(
    target.replicas.every((replica) => replica.binarySha256 === target.binarySha256),
    `${target.target} binary reproducibility drift`,
  );
}
const r6ReproducibilityAsset = r6Manifest.assets.find(
  (asset) => asset.name === 'REPRODUCIBILITY.json',
);
assert.ok(r6ReproducibilityAsset, 'R6 manifest lacks reproducibility evidence');
assert.equal(r6ReproducibilityAsset.bytes, r6ReproducibilityBytes.length);
assert.equal(
  r6ReproducibilityAsset.sha256,
  createHash('sha256').update(r6ReproducibilityBytes).digest('hex'),
  'R6 reproducibility evidence hash drift',
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
for (const [name, source] of Object.entries(installerSources)) {
  assert.ok(hasCanonicalReleaseBase(source), `${name} installer lacks the GitHub release base`);
  assert.match(source, /release selector is required/iu, `${name} installer must require a release selector`);
  assert.match(source, /channel latest/iu, `${name} installer must expose the explicit latest channel`);
  assert.ok(source.includes('.sha256'), `${name} installer must fetch a checksum sidecar`);
  assert.match(source, /sha256 mismatch/iu, `${name} installer must fail on checksum mismatch`);
  assert.match(source, /Verified Ed25519 release manifest/iu, `${name} installer must verify the signed manifest internally`);
  assert.ok(source.includes('97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250'), `${name} installer release fingerprint drift`);
  assert.ok(source.includes('RELEASE-MANIFEST.json.sig'), `${name} installer lacks detached signature verification`);
  assert.doesNotMatch(source, /api\.github\.com|latest\.txt|dl\.pliego\.run/iu);
}

function hasCanonicalReleaseBase(source) {
  const candidates = source.match(/https:\/\/[^"'\s]+/gu) ?? [];
  return candidates.some((candidate) => {
    let url;
    try {
      url = new URL(candidate);
    } catch {
      return false;
    }
    return url.protocol === 'https:' &&
      url.hostname === 'github.com' &&
      url.port === '' &&
      url.username === '' &&
      url.password === '' &&
      url.pathname === '/celiumsai/pliegors/releases/download' &&
      url.search === '' &&
      url.hash === '';
  });
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
assert.ok(cli.includes('fn registry_dependency()'), 'starter registry dependency builder');
assert.ok(cli.includes('version = \\\"={}\\\"'), 'starter dependency must use an exact registry version');
assert.ok(cli.includes('crates.io PliegoRS'), 'starter source label must name crates.io');
assert.ok(!cli.includes('git = \\\"{PLIEGORS_SOURCE_REPOSITORY}'), 'released starter must not depend on Git');
assert.equal(existsSync(path.join(root, 'crates/pliego-cli/build.rs')), false, 'CLI must build from crates.io without Git metadata');

console.log(
  `Distribution contract PASS: ${crates.length} crates.io packages across ` +
  `${[...new Set(crates.map((pkg) => pkg.version))].sort().join(', ')}, ` +
  '5 targets x 2 replicas, signed release candidate, and gated manual draft',
);
