#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const schema = await readJson('schemas/pliego.product-capabilities.schema.json');
const manifest = await readJson('product.capabilities.json');
const cargo = await readText('Cargo.toml');
const packageDocument = await readJson('package.json');
const toolchain = await readText('rust-toolchain.toml');
const sdkCargo = await readText('crates/pliego-sdk/Cargo.toml');
const readme = await readText('README.md');
const framework = await readText('FRAMEWORK.md');
const constitution = await readText('docs/34-product-constitution.md');
const capabilityDocs = await readText('docs/47-product-capability-manifest.md');
const siteMain = await readText('examples/pliegors-site/src/main.rs');
const sitePages = await readText('examples/pliegors-site/src/pages.rs');
const siteDocs = await readText('examples/pliegors-site/src/docs.rs');

const validate = new Ajv2020({ allErrors: true, strict: true }).compile(schema);
assert.equal(validate(manifest), true, formatErrors(validate.errors));

const invalidStability = structuredClone(manifest);
invalidStability.surfaces.find((surface) => surface.availability === 'not-released').stability = 'preview';
assert.equal(validate(invalidStability), false, 'schema accepts a stability promise for a non-released surface');

const invalidReleased = structuredClone(manifest);
invalidReleased.surfaces.find((surface) => surface.availability === 'released').stability = 'none';
assert.equal(validate(invalidReleased), false, 'schema accepts a released surface without a compatibility tier');

const workspaceVersion = tomlSectionString(cargo, 'workspace.package', 'version');
const workspaceRust = tomlSectionString(cargo, 'workspace.package', 'rust-version');
const toolchainVersion = tomlSectionString(toolchain, 'toolchain', 'channel');
const sdkVersion = tomlSectionString(sdkCargo, 'package', 'version');

assert.equal(manifest.framework.workspaceVersion, workspaceVersion, 'workspace version drift');
assert.equal(manifest.framework.releasedVersion, packageDocument.version, 'released npm metadata version drift');
assert.equal(manifest.framework.openSdkVersion, sdkVersion, 'OpenSDK version drift');
assert.equal(manifest.support.rust.msrv, `${workspaceRust}.0`, 'MSRV drift');
assert.equal(manifest.support.rust.releaseToolchain, toolchainVersion, 'release toolchain drift');

const crateMembers = workspaceArray(cargo, 'members').filter((member) => member.startsWith('crates/'));
const crateNames = [];
for (const member of crateMembers) {
  const memberCargo = await readText(path.join(member, 'Cargo.toml'));
  crateNames.push(tomlSectionString(memberCargo, 'package', 'name'));
}
const unreleased = new Set(manifest.framework.unreleasedCrates);
const releasedCrates = crateNames.filter((name) => !unreleased.has(name));
assert.equal(releasedCrates.length, manifest.framework.releasedCrateCount, 'released crate count drift');
assert.deepEqual(
  [...crateNames].sort(),
  [...new Set(crateNames)].sort(),
  'workspace contains duplicate crate package names',
);
for (const name of unreleased) {
  assert.ok(crateNames.includes(name), `unreleased crate is not a workspace crate: ${name}`);
}

const surfaceIds = manifest.surfaces.map((surface) => surface.id);
assert.equal(new Set(surfaceIds).size, surfaceIds.length, 'duplicate capability surface ID');
const targetIds = manifest.support.targets.map((target) => target.id);
assert.equal(new Set(targetIds).size, targetIds.length, 'duplicate target ID');
const browserIds = manifest.support.browsers.map((browser) => browser.id);
assert.equal(new Set(browserIds).size, browserIds.length, 'duplicate browser ID');

for (const relativePath of new Set([
  ...manifest.surfaces.flatMap((surface) => surface.evidence),
  ...manifest.support.browsers.map((browser) => browser.evidence),
])) {
  const resolved = path.resolve(root, relativePath);
  assert.ok(isInside(root, resolved), `evidence escapes repository: ${relativePath}`);
  await access(resolved);
}

const requiredSurfaces = new Map([
  ['deterministic-ssg', ['released', 'preview', 'preserved']],
  ['rust-wasm-ui', ['released', 'preview', 'preserved']],
  ['native-http-runtime', ['not-released', 'none', 'G1']],
  ['dynamic-ssr', ['not-released', 'none', 'G1']],
  ['fullstack-routing', ['not-released', 'none', 'G1']],
  ['data-actions-cache', ['not-released', 'none', 'G2']],
  ['pboc', ['not-released', 'none', 'G3']],
  ['cloudflare-runtime', ['not-released', 'none', 'G3']],
  ['opensdk-server', ['not-released', 'none', 'G5']],
]);
for (const [id, expected] of requiredSurfaces) {
  const surface = manifest.surfaces.find((candidate) => candidate.id === id);
  assert.ok(surface, `required product surface missing: ${id}`);
  assert.deepEqual(
    [surface.availability, surface.stability, surface.gate],
    expected,
    `product surface drift: ${id}`,
  );
}

const publicTruth = [
  ['README.md', readme],
  ['FRAMEWORK.md', framework],
  ['docs/34-product-constitution.md', constitution],
  ['docs/47-product-capability-manifest.md', capabilityDocs],
];
for (const [name, content] of publicTruth) {
  for (const stale of ['v0.0.1', 'PliegoRS 0.0.1', 'Rust 1.85 is the current minimum']) {
    assert.ok(!content.includes(stale), `${name} contains stale product statement: ${stale}`);
  }
}

for (const [name, content, required] of [
  ['README.md', readme, ['product.capabilities.json', '0.0.2', '0.1.0-preview.1', 'Rust `1.86`']],
  ['FRAMEWORK.md', framework, ['product.capabilities.json', 'deterministic static sites', 'Streaming SSR']],
  ['docs/34-product-constitution.md', constitution, ['product.capabilities.json', 'Linux x64 and ARM64', 'Chromium']],
  ['website pages', sitePages, ['0.0.2 is public on crates.io', 'R0-R7 and P8 evidence']],
  ['website docs', siteDocs, ['/capabilities.json', 'OpenSDK 0.1.0-preview.1', 'Rust 1.86']],
]) {
  for (const token of required) assert.ok(content.includes(token), `${name} lacks product-truth token: ${token}`);
}

for (const token of [
  'Asset::new(',
  '"capabilities.json"',
  'include_bytes!("../../../product.capabilities.json")',
]) assert.ok(siteMain.includes(token), `official site does not publish canonical capability manifest: ${token}`);

assert.ok(
  capabilityDocs.includes('https://pliegors.dev/capabilities.json'),
  'capability documentation lacks the public manifest URL',
);
for (const id of requiredSurfaces.keys()) {
  assert.ok(manifest.surfaces.some((surface) => surface.id === id), `manifest lost ${id}`);
}

process.stdout.write(
  [
    'Product truth: PASS',
    `release ${manifest.framework.releasedVersion}`,
    `workspace ${manifest.framework.workspaceVersion}`,
    `OpenSDK ${manifest.framework.openSdkVersion}`,
    `${releasedCrates.length} released crates`,
    `${manifest.surfaces.length} surfaces`,
    `${manifest.support.targets.length} release targets`,
  ].join(' | ') + '\n',
);

async function readJson(relativePath) {
  return JSON.parse(await readText(relativePath));
}

async function readText(relativePath) {
  return readFile(path.join(root, relativePath), 'utf8');
}

function tomlSectionString(source, section, key) {
  const body = tomlSection(source, section);
  const keyMatch = body.match(new RegExp(`^${key}\\s*=\\s*"([^"]+)"\\s*$`, 'mu'));
  assert.ok(keyMatch, `missing TOML key ${section}.${key}`);
  return keyMatch[1];
}

function workspaceArray(source, key) {
  const workspace = tomlSection(source, 'workspace');
  const array = workspace.match(new RegExp(`^${key}\\s*=\\s*\\[([\\s\\S]*?)\\]`, 'mu'));
  assert.ok(array, `missing workspace array ${key}`);
  return [...array[1].matchAll(/"([^"]+)"/gu)].map((match) => match[1]);
}

function tomlSection(source, section) {
  const marker = `[${section}]`;
  const markerStart = source.indexOf(marker);
  assert.notEqual(markerStart, -1, `missing TOML section ${marker}`);
  const bodyStart = markerStart + marker.length;
  const remainder = source.slice(bodyStart);
  const nextSection = remainder.search(/^\[/mu);
  return nextSection === -1 ? remainder : remainder.slice(0, nextSection);
}

function isInside(parent, child) {
  const relative = path.relative(parent, child);
  return relative !== '' && !relative.startsWith('..') && !path.isAbsolute(relative);
}

function formatErrors(errors) {
  return errors?.map((error) => `${error.instancePath || '/'} ${error.message}`).join('\n') ?? '';
}
