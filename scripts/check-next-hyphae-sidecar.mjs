#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-only

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = path.join(root, "fixtures", "next", "hyphae-console");
const manifestPath = path.join(root, "fixtures", "next", "manifest.json");
const authorityPath = path.join(fixtureRoot, "sidecar-authority.json");
const cargoPath = path.join(fixtureRoot, "Cargo.toml");

const [manifest, authority] = await Promise.all([
  readJson(manifestPath),
  readJson(authorityPath),
]);
const fixture = manifest.fixtures.find((candidate) => candidate.id === "hyphae-console");
assert(fixture, "Hyphae Console fixture is missing");
const descriptor = await readJson(path.join(fixtureRoot, "fixture.json"));
assert.equal(descriptor.stage, "specified", "Hyphae Console was promoted before acceptance");
assert.equal(fixture.externalAuthority.repository, authority.repository);
assert.equal(fixture.externalAuthority.releaseTag, authority.releaseTag);
assert.equal(fixture.externalAuthority.releaseRevision, authority.releaseRevision);
assert.equal(fixture.externalAuthority.releaseChecksumsSha256, authority.releaseChecksumsSha256);
assert.equal(fixture.externalAuthority.transport, authority.transport);
assert.equal(fixture.externalAuthority.productMediaType, authority.productMediaType);
assert.equal(fixture.externalAuthority.errorMediaType, authority.errorMediaType);
assert.equal(fixture.externalAuthority.rustMsrv, authority.rustMsrv);

const metadata = JSON.parse(execFileSync("cargo", [
  "metadata",
  "--manifest-path", cargoPath,
  "--locked",
  "--format-version", "1",
], { cwd: root, encoding: "utf8" }));
const forbidden = metadata.packages
  .filter((pkg) => pkg.name.startsWith("hyphae-"))
  .map((pkg) => `${pkg.name}@${pkg.version}`);
assert.deepEqual(forbidden, [], `Hyphae crates entered the fixture graph: ${forbidden.join(", ")}`);

const fixturePackage = metadata.packages.find((pkg) => pkg.name === "pliegors-next-hyphae-console-server");
assert(fixturePackage, "private Hyphae Console server package is missing");
assert.deepEqual(fixturePackage.publish, [], "Hyphae Console server became publishable");
assert.equal(fixturePackage.rust_version, "1.86", "Hyphae Console server MSRV drift");
assert.equal(
  fixturePackage.targets.some((target) => target.kind.includes("bin")),
  false,
  "Hyphae Console introduced a product binary before fixture promotion",
);
assert.equal(
  fixturePackage.dependencies.some((dependency) => dependency.name === "pliego-hyphae"),
  false,
  "Hyphae Console conflates Native Product v2 with verified-sync v2",
);

process.stdout.write(
  `Hyphae sidecar contract PASS: ${authority.releaseTag} ${authority.transport}, ${metadata.packages.length} packages, no hyphae-* dependencies\n`,
);

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}
