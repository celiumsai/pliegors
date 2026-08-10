#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { readFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  buildInventoryReport,
  loadNextBaselineContracts,
  publishBaselineDirectory,
  renderBaselineMarkdown,
  validateBaselineReport,
  validateFixtureManifest,
} from "./next-baseline-lib.mjs";
import {
  applyFixtureBuildMeasurements,
  collectFixtureBuildSamples,
  prepareNextBaselineCli,
  projectFixtureBuildMetrics,
} from "./next-baseline-build.mjs";
import {
  applyFixtureDevMeasurements,
  collectFixtureDevSamples,
  projectFixtureDevMetrics,
} from "./next-baseline-dev.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export async function measureNextBaseline({
  repositoryRoot = root,
  manifestPath = path.join(repositoryRoot, "fixtures", "next", "manifest.json"),
  outputDirectory = path.join(repositoryRoot, "target", "benchmarks", "next-baseline-inventory"),
  executeMinimal = false,
  executeStressDashboard = false,
  executeDevHmr = false,
  keep = false,
} = {}) {
  const contracts = await loadNextBaselineContracts(repositoryRoot);
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  await validateFixtureManifest(manifest, contracts, repositoryRoot);
  let report = await buildInventoryReport({ manifest, contracts, root: repositoryRoot });
  const selectedIds = [
    ...(executeMinimal ? ["minimal"] : []),
    ...(executeStressDashboard ? ["stress-dashboard"] : []),
  ];
  let cli = null;
  if (selectedIds.length > 0) {
    cli = prepareNextBaselineCli(repositoryRoot);
  }
  if (selectedIds.length > 0) {
    const executions = [];
    for (const fixture of manifest.fixtures.filter((candidate) => selectedIds.includes(candidate.id))) {
      const samples = await collectFixtureBuildSamples({ root: repositoryRoot, fixture, manifest, cli, keep });
      const metrics = projectFixtureBuildMetrics(samples, manifest);
      executions.push({ fixtureId: fixture.id, metrics, peakRssMeasured: metrics.some((metric) => metric.caseId === "peak-rss") });
    }
    report = applyFixtureBuildMeasurements(report, executions);
    report.completedAt = new Date().toISOString();
  }
  if (executeDevHmr) {
    if (selectedIds.length === 0) throw new Error("--execute-dev-hmr requires --execute-minimal or --execute-stress-dashboard");
    cli ??= prepareNextBaselineCli(repositoryRoot);
    const executions = [];
    for (const fixture of manifest.fixtures.filter((candidate) => selectedIds.includes(candidate.id))) {
      const collected = await collectFixtureDevSamples({ root: repositoryRoot, fixture, manifest, cli, keep });
      executions.push({
        fixtureId: fixture.id,
        metrics: projectFixtureDevMetrics(collected.samples, manifest),
        browser: collected.browser,
      });
    }
    report = applyFixtureDevMeasurements(report, executions);
    report.completedAt = new Date().toISOString();
  }
  await validateBaselineReport(report, manifest, contracts, repositoryRoot);
  const markdown = renderBaselineMarkdown(report, manifest);
  await publishBaselineDirectory(outputDirectory, report, markdown);
  return { manifest, report, outputDirectory };
}

function parseOptions(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 1) {
    const option = args[index];
    if (option === "--help") return { help: true };
    if (["--execute-minimal", "--execute-stress-dashboard", "--execute-dev-hmr", "--keep"].includes(option)) {
      if (Object.hasOwn(options, option)) throw new Error(`duplicate option: ${option}`);
      options[option] = true;
      continue;
    }
    if (!["--manifest", "--output"].includes(option)) throw new Error(`unknown option: ${option}`);
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
    process.stdout.write("Usage: node scripts/measure-next-baseline.mjs [--manifest <file>] [--output <new-directory>] [--execute-minimal] [--execute-stress-dashboard] [--execute-dev-hmr] [--keep]\n");
    return;
  }
  const { report, outputDirectory } = await measureNextBaseline({
    manifestPath: options["--manifest"] ? path.resolve(options["--manifest"]) : undefined,
    outputDirectory: options["--output"] ? path.resolve(options["--output"]) : undefined,
    executeMinimal: options["--execute-minimal"] === true,
    executeStressDashboard: options["--execute-stress-dashboard"] === true,
    executeDevHmr: options["--execute-dev-hmr"] === true,
    keep: options["--keep"] === true,
  });
  process.stdout.write(`Next baseline inventory ${report.status}: ${outputDirectory}\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error}\n`);
    process.exitCode = 1;
  });
}
