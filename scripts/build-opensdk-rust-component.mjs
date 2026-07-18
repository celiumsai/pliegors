#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const guest = path.join(root, "tools", "opensdk", "rust-component-transform");
const componentizer = path.join(root, "tools", "opensdk", "componentize");
const core = path.join(
  guest,
  "target",
  "wasm32-unknown-unknown",
  "release",
  "pliego_opensdk_rust_component_transform.wasm",
);
const output = path.join(root, "target", "opensdk", "rust-component-transform.wasm");

run("cargo", [
  "build",
  "--manifest-path",
  path.join(guest, "Cargo.toml"),
  "--target",
  "wasm32-unknown-unknown",
  "--release",
  "--locked",
]);
run("cargo", [
  "run",
  "--manifest-path",
  path.join(componentizer, "Cargo.toml"),
  "--release",
  "--locked",
  "--",
  core,
  output,
]);
process.stdout.write(`${output}\n`);

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
