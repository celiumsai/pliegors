#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-only

import { readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { fixtureTreeIdentity } from "./next-baseline-lib.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export async function updateNextFixtureIdentities({ repositoryRoot = root } = {}) {
  const manifestPath = path.join(repositoryRoot, "fixtures", "next", "manifest.json");
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  for (const fixture of manifest.fixtures) {
    fixture.sourceIdentity = await fixtureTreeIdentity(
      path.resolve(repositoryRoot, ...fixture.root.split("/")),
    );
  }
  const temporaryPath = `${manifestPath}.tmp`;
  await writeFile(temporaryPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  await rename(temporaryPath, manifestPath);
  return manifest;
}

async function main() {
  if (process.argv.length > 2) throw new Error("update-next-fixture-identities accepts no options");
  const manifest = await updateNextFixtureIdentities();
  process.stdout.write(`Next fixture identities updated: ${manifest.fixtures.length} fixtures\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error}\n`);
    process.exitCode = 1;
  });
}
