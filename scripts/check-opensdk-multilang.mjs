#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { transformSync } from "esbuild";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const input = await readFile(
  path.join(root, "fixtures", "opensdk", "multilang-transform", "input.json"),
);
const python = available("python3") ? "python3" : available("python") ? "python" : null;
if (!python) throw new Error("Python 3 is required for OpenSDK multilang conformance");
const typescriptRuntime = transformSync(
  readFileSync(path.join(root, "tools", "opensdk", "typescript", "transform.ts"), "utf8"),
  {
    format: "esm",
    legalComments: "none",
    loader: "ts",
    sourcemap: false,
    target: "node20",
  },
).code;

const implementations = new Map([
  ["rust", ["cargo", [
    "run",
    "--quiet",
    "--locked",
    "--manifest-path",
    "tools/opensdk/rust-transform/Cargo.toml",
  ]]],
  ["typescript", ["node", ["--input-type=module", "--eval", typescriptRuntime]]],
  ["python", [python, ["tools/opensdk/python/transform.py"]]],
]);
const outputs = new Map();
for (const [language, [command, argumentsList]] of implementations) {
  const first = run(command, argumentsList, input);
  const second = run(command, argumentsList, input);
  assert.equal(first, second, `${language} transform is not byte deterministic`);
  outputs.set(language, first);
}
const firstComponent = runComponent();
const secondComponent = runComponent();
assert.equal(firstComponent, secondComponent, "Rust Component transform is not byte deterministic");
outputs.set("rust-component", firstComponent);

const [authority, ...others] = outputs.values();
for (const output of others) assert.equal(output, authority, "toolchain outputs differ");
const value = JSON.parse(authority);
assert.deepEqual(value, {
  schema: "dev.pliegors.build-transform/v1",
  mediaType: "text/plain; charset=utf-8",
  bytesBase64: Buffer.from("PliegoRS: ACCOUNTABLE WEB PLATFORM").toString("base64"),
});
process.stdout.write(
  `OpenSDK multilang PASS: ${[...outputs.keys()].join(", ")} -> ${value.bytesBase64}\n`,
);

function available(command) {
  const result = spawnSync(command, ["--version"], { cwd: root, windowsHide: true });
  return !result.error && result.status === 0;
}

function run(command, argumentsList, stdin) {
  const result = spawnSync(command, argumentsList, {
    cwd: root,
    input: stdin,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${argumentsList.join(" ")} failed:\n${result.stderr}`);
  }
  return result.stdout.trim();
}

function runComponent() {
  run("node", ["scripts/check-opensdk-component-runtime.mjs"]);
  const report = JSON.parse(
    readFileSync(
      path.join(root, "target", "opensdk", "component-runtime-report.json"),
      "utf8",
    ),
  );
  return JSON.stringify({
    schema: "dev.pliegors.build-transform/v1",
    mediaType: report.buildTransform.output.mediaType,
    bytesBase64: Buffer.from(report.buildTransform.output.bytes).toString("base64"),
  });
}
