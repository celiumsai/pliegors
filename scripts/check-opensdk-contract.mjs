#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const schemaFiles = [
  "pliego.sdk-extension.schema.json",
  "pliego.sdk-admission.schema.json",
  "pliego.effect-receipt.schema.json",
  "pliego.sdk-compatibility-matrix.schema.json",
  "pliego.build-transform-receipt.schema.json",
];
const ajv = new Ajv2020({ allErrors: true, strict: true });
for (const file of schemaFiles) {
  ajv.addSchema(JSON.parse(await readFile(path.join(root, "schemas", file), "utf8")));
}

const fixtureRoot = path.join(root, "fixtures", "opensdk", "browser-component");
const manifestPath = path.join(fixtureRoot, "pliego-extension.json");
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
validate("https://pliegors.dev/schemas/pliego.sdk-extension.schema.json", manifest, manifestPath);
const manifestValidator = ajv.getSchema(
  "https://pliegors.dev/schemas/pliego.sdk-extension.schema.json",
);
expectInvalid(manifestValidator, {
  ...manifest,
  entry: { ...manifest.entry, customElement: "div" },
}, "non-Custom-Element name");
const { imports: _omittedImports, ...missingImports } = manifest;
expectInvalid(manifestValidator, missingImports, "missing imports contract vector");
expectInvalid(manifestValidator, {
  ...manifest,
  determinism: "pure",
}, "pure extension with capabilities");
expectInvalid(manifestValidator, {
  ...manifest,
  lifecycle: { ...manifest.lifecycle, update: false },
}, "HMR lifecycle without update");
expectInvalid(manifestValidator, {
  ...manifest,
  plane: "build",
}, "build plane with browser entry");
const effectReceiptValidator = ajv.getSchema(
  "https://pliegors.dev/schemas/pliego.effect-receipt.schema.json",
);
validate(
  "https://pliegors.dev/schemas/pliego.effect-receipt.schema.json",
  {
    schema: "dev.pliegors.effect-receipt/v1",
    sequence: 1,
    outcome: "error",
    capability: "network",
    operation: "fetch",
    inputSha256: `sha256:${"0".repeat(64)}`,
    outputSha256: `sha256:${"f".repeat(64)}`,
  },
  "brokered failure receipt",
);
expectInvalid(effectReceiptValidator, {
  schema: "dev.pliegors.effect-receipt/v1",
  sequence: 4097,
  outcome: "success",
  capability: "network",
  operation: "contains space",
  inputSha256: `sha256:${"0".repeat(64)}`,
  outputSha256: `sha256:${"f".repeat(64)}`,
}, "unbounded non-canonical effect receipt");

const entryPath = path.join(fixtureRoot, manifest.entry.path);
const entry = await readFile(entryPath);
const digest = `sha256:${createHash("sha256").update(entry).digest("hex")}`;
if (digest !== manifest.identity.digest) {
  throw new Error(`OpenSDK fixture digest mismatch: ${digest} != ${manifest.identity.digest}`);
}
const browserModule = await import(`${pathToFileURL(entryPath).href}?digest=${digest.slice(7)}`);
assert.deepEqual(Object.keys(browserModule).sort(), manifest.exports);
assert.equal(browserModule.pliegoComponent.apiVersion, manifest.apiVersion);
assert.equal(browserModule.pliegoComponent.tagName, manifest.entry.customElement);
assert.deepEqual([...browserModule.pliegoComponent.capabilities].sort(), manifest.capabilities);

const result = spawnSync(
  "cargo",
  [
    "run",
    "--locked",
    "-q",
    "-p",
    "pliego-cli",
    "--",
    "sdk",
    "check",
    manifestPath,
    "--grant",
    "dom",
    "--format",
    "json",
  ],
  { cwd: root, encoding: "utf8", windowsHide: true },
);
if (result.error) throw result.error;
if (result.status !== 0) {
  throw new Error(`pliego sdk check failed:\n${result.stderr}`);
}
const report = JSON.parse(result.stdout);
if (
  report.contract !== "dev.pliegors.sdk-conformance/v1" ||
  report.level !== "admission" ||
  report.result !== "pass"
) {
  throw new Error("pliego sdk check emitted an incompatible conformance report");
}
validate(
  "https://pliegors.dev/schemas/pliego.sdk-admission.schema.json",
  report.admission,
  "CLI admission receipt",
);
if (report.entrySha256 !== digest || report.admission.digest !== digest) {
  throw new Error("CLI evidence does not bind the exact fixture bytes");
}

const compatibility = spawnSync(
  "cargo",
  [
    "run",
    "--locked",
    "-q",
    "-p",
    "pliego-cli",
    "--",
    "sdk",
    "compatibility",
    "--format",
    "json",
  ],
  { cwd: root, encoding: "utf8", windowsHide: true },
);
if (compatibility.error) throw compatibility.error;
if (compatibility.status !== 0) {
  throw new Error(`pliego sdk compatibility failed:\n${compatibility.stderr}`);
}
validate(
  "https://pliegors.dev/schemas/pliego.sdk-compatibility-matrix.schema.json",
  JSON.parse(compatibility.stdout),
  "CLI compatibility matrix",
);

process.stdout.write(
  `OpenSDK contract PASS: ${schemaFiles.length} schemas, 7 WIT packages, ` +
    `${report.checks.length} admission checks, ${digest}\n`,
);

function validate(schemaId, value, label) {
  const validator = ajv.getSchema(schemaId);
  if (!validator) throw new Error(`missing schema ${schemaId}`);
  if (!validator(value)) {
    throw new Error(`${label} fails ${schemaId}: ${ajv.errorsText(validator.errors)}`);
  }
}

function expectInvalid(validator, value, label) {
  if (!validator) throw new Error(`missing validator for ${label}`);
  if (validator(value)) {
    throw new Error(`OpenSDK schema accepted ${label}`);
  }
}
