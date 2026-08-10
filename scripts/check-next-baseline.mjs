#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { lstat, readdir, readFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  loadNextBaselineContracts,
  renderBaselineMarkdown,
  validateBaselineReport,
  validateFixtureManifest,
} from "./next-baseline-lib.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export async function checkNextBaseline({
  repositoryRoot = root,
  manifestPath = path.join(repositoryRoot, "fixtures", "next", "manifest.json"),
  baselineDirectory = path.join(repositoryRoot, "benchmarks", "baselines", "next"),
} = {}) {
  const contracts = await loadNextBaselineContracts(repositoryRoot);
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  await validateFixtureManifest(manifest, contracts, repositoryRoot);

  const directoryDetails = await lstat(baselineDirectory);
  if (!directoryDetails.isDirectory() || directoryDetails.isSymbolicLink()) {
    throw new Error("baseline path is not a regular directory");
  }
  const entries = await readdir(baselineDirectory, { withFileTypes: true });
  const names = entries.map((entry) => entry.name).sort();
  if (JSON.stringify(names) !== JSON.stringify(["baseline.json", "baseline.md"])) {
    throw new Error("baseline directory must contain exactly baseline.json and baseline.md");
  }
  if (entries.some((entry) => !entry.isFile() || entry.isSymbolicLink())) {
    throw new Error("baseline outputs must be regular files");
  }
  const jsonPath = path.join(baselineDirectory, "baseline.json");
  const markdownPath = path.join(baselineDirectory, "baseline.md");
  const report = JSON.parse(await readFile(jsonPath, "utf8"));
  await validateBaselineReport(report, manifest, contracts, repositoryRoot);
  const actualMarkdown = await readFile(markdownPath, "utf8");
  const expectedMarkdown = renderBaselineMarkdown(report, manifest);
  if (actualMarkdown !== expectedMarkdown) {
    throw new Error("baseline.md is not the deterministic rendering of baseline.json");
  }
  return { manifest, report };
}

function parseOptions(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 1) {
    const option = args[index];
    if (option === "--help") return { help: true };
    if (!["--manifest", "--baseline"].includes(option)) throw new Error(`unknown option: ${option}`);
    if (Object.hasOwn(options, option)) throw new Error(`duplicate option: ${option}`);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`${option} requires a path`);
    options[option] = value;
    index += 1;
  }
  return options;
}

async function main() {
  const options = parseOptions(process.argv.slice(2));
  if (options.help) {
    process.stdout.write("Usage: node scripts/check-next-baseline.mjs [--manifest <file>] [--baseline <directory>]\n");
    return;
  }
  const { manifest, report } = await checkNextBaseline({
    manifestPath: options["--manifest"] ? path.resolve(options["--manifest"]) : undefined,
    baselineDirectory: options["--baseline"] ? path.resolve(options["--baseline"]) : undefined,
  });
  process.stdout.write(`Next baseline contract PASS: ${manifest.fixtures.length} fixtures, ${manifest.measurementCases.length} cases, status ${report.status}\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error}\n`);
    process.exitCode = 1;
  });
}
