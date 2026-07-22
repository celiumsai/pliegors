#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
run("node", [path.join(root, "scripts", "build-opensdk-rust-component.mjs")]);
const fixtureRoot = path.join(root, "target", "opensdk");
const componentPath = path.join(fixtureRoot, "rust-component-transform.wasm");
const manifestPath = path.join(fixtureRoot, "rust-component-transform.json");
const inputPath = path.join(fixtureRoot, "rust-component-input.json");
const component = await readFile(componentPath);
const digest = `sha256:${createHash("sha256").update(component).digest("hex")}`;
const manifest = {
  schema: "dev.pliegors.sdk-extension/v1",
  apiVersion: "0.2.0-beta.1",
  hostVersion: ">=0.2.0-beta.1, <0.3.0",
  plane: "build",
  identity: {
    namespace: "pliego",
    name: "uppercase-component",
    version: "0.1.0",
    digest,
  },
  entry: {
    kind: "wasm-component",
    path: path.basename(componentPath),
    world: "pliego:build/transformer@0.1.0",
  },
  determinism: "pure",
  imports: [],
  exports: ["pliego:build/transform@0.1.0"],
  capabilities: [],
  requiredFeatures: [],
  optionalFeatures: [],
  budgets: {
    cpuMs: 100,
    wallTimeMs: 1000,
    memoryBytes: 64 * 1024 * 1024,
    outputBytes: 1024 * 1024,
  },
  lifecycle: {
    init: true,
    update: false,
    suspend: false,
    resume: false,
    dispose: true,
    hmr: false,
  },
};
const source = "accountable web platform";
const prefix = "PliegoRS: ";
const input = {
  path: "input.txt",
  mediaType: "text/plain; charset=utf-8",
  bytes: [...Buffer.from(source, "utf8")],
  optionsJson: JSON.stringify({ prefix }),
};
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
await writeFile(inputPath, `${JSON.stringify(input, null, 2)}\n`);

const result = run("cargo", [
  "run",
  "--locked",
  "-q",
  "-p",
  "pliego-cli",
  "--",
  "sdk",
  "test",
  manifestPath,
  "--input",
  inputPath,
  "--format",
  "json",
], true);
const report = JSON.parse(result.stdout);
await writeFile(
  path.join(fixtureRoot, "component-runtime-report.json"),
  `${JSON.stringify(report, null, 2)}\n`,
);
assert.equal(report.level, "build-transform-execution");
assert(report.checks.includes("typed-build-transform"));
assert(report.checks.includes("runtime-budgets"));
assert.equal(
  Buffer.from(report.buildTransform.output.bytes).toString("utf8"),
  `${prefix}${source.toUpperCase()}`,
);
assert.equal(report.buildTransform.output.diagnosticsJson, "[]");
assert.equal(report.buildTransform.receipt.extensionDigest, digest);
assert.equal(
  report.buildTransform.receipt.outputSha256,
  hashFramed([
    Buffer.from(report.buildTransform.output.mediaType, "utf8"),
    Buffer.from(report.buildTransform.output.bytes),
    Buffer.from(report.buildTransform.output.diagnosticsJson, "utf8"),
  ]),
);
assert(report.buildTransform.receipt.fuelConsumed > 0);

const overBudgetManifest = {
  ...manifest,
  budgets: { ...manifest.budgets, outputBytes: 48 },
};
const overBudgetPath = path.join(fixtureRoot, "rust-component-over-budget.json");
await writeFile(overBudgetPath, `${JSON.stringify(overBudgetManifest, null, 2)}\n`);
const overBudget = run("cargo", [
  "run",
  "--locked",
  "-q",
  "-p",
  "pliego-cli",
  "--",
  "sdk",
  "test",
  overBudgetPath,
  "--input",
  inputPath,
  "--format",
  "json",
], true, false);
assert.equal(overBudget.status, 9);
assert.match(overBudget.stderr, /transform output exceeds the 48-byte budget/);

const deadlineManifest = {
  ...manifest,
  budgets: { ...manifest.budgets, cpuMs: 60_000, wallTimeMs: 100 },
};
const deadlineManifestPath = path.join(fixtureRoot, "rust-component-deadline.json");
const deadlineInputPath = path.join(fixtureRoot, "rust-component-deadline-input.json");
await writeFile(deadlineManifestPath, `${JSON.stringify(deadlineManifest, null, 2)}\n`);
await writeFile(
  deadlineInputPath,
  `${JSON.stringify({
    ...input,
    optionsJson: JSON.stringify({ prefix, mode: "spin" }),
  }, null, 2)}\n`,
);
const deadline = run("cargo", [
  "run",
  "--locked",
  "-q",
  "-p",
  "pliego-cli",
  "--",
  "sdk",
  "test",
  deadlineManifestPath,
  "--input",
  deadlineInputPath,
  "--format",
  "json",
], true, false);
assert.equal(deadline.status, 9);
assert.match(deadline.stderr, /component exceeded its wall-time budget/);

const fuelManifest = {
  ...manifest,
  budgets: { ...manifest.budgets, cpuMs: 1, wallTimeMs: 300_000 },
};
const fuelManifestPath = path.join(fixtureRoot, "rust-component-fuel.json");
await writeFile(fuelManifestPath, `${JSON.stringify(fuelManifest, null, 2)}\n`);
const fuel = run("cargo", [
  "run",
  "--locked",
  "-q",
  "-p",
  "pliego-cli",
  "--",
  "sdk",
  "test",
  fuelManifestPath,
  "--input",
  deadlineInputPath,
  "--format",
  "json",
], true, false);
assert.equal(fuel.status, 9);
assert.match(fuel.stderr, /component exhausted its fuel budget/);

const memoryManifest = {
  ...manifest,
  budgets: { ...manifest.budgets, memoryBytes: 64 * 1024 },
};
const memoryManifestPath = path.join(fixtureRoot, "rust-component-memory.json");
await writeFile(memoryManifestPath, `${JSON.stringify(memoryManifest, null, 2)}\n`);
const memory = run("cargo", [
  "run",
  "--locked",
  "-q",
  "-p",
  "pliego-cli",
  "--",
  "sdk",
  "test",
  memoryManifestPath,
  "--input",
  inputPath,
  "--format",
  "json",
], true, false);
assert.equal(memory.status, 9);
assert.match(memory.stderr, /memory|resource limit/i);

const schema = JSON.parse(
  await readFile(
    path.join(root, "schemas", "pliego.build-transform-receipt.schema.json"),
    "utf8",
  ),
);
const validate = new Ajv2020({ allErrors: true, strict: true }).compile(schema);
assert(
  validate(report.buildTransform.receipt),
  `invalid build transform receipt: ${JSON.stringify(validate.errors)}`,
);
process.stdout.write(
  `OpenSDK component runtime PASS: typed WIT transform, fuel, deadline, memory/output budgets, ${digest}\n`,
);

function hashFramed(values) {
  const digest = createHash("sha256");
  for (const value of values) {
    const length = Buffer.alloc(8);
    length.writeBigUInt64LE(BigInt(value.length));
    digest.update(length).update(value);
  }
  return `sha256:${digest.digest("hex")}`;
}

function run(command, args, capture = false, requireSuccess = true) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    stdio: capture ? "pipe" : "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (requireSuccess && result.status !== 0) {
    throw new Error(`${command} failed (${result.status}):\n${result.stderr ?? ""}`);
  }
  return result;
}
