#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const build = spawnSync("cargo", ["build", "--locked", "-q", "-p", "pliego-cli"], {
  cwd: root,
  encoding: "utf8",
  windowsHide: true,
});
if (build.error) throw build.error;
if (build.status !== 0) throw new Error(`cannot build tooling host:\n${build.stderr}`);
const binary = path.join(root, "target", "debug", process.platform === "win32" ? "pliego.exe" : "pliego");

async function checkEditorClient() {
  const session = new Session("pliego");
  try {
    const premature = await session.request({
      jsonrpc: "2.0",
      id: "editor-premature",
      method: "pliego/diagnostics",
    });
    assert.equal(premature.error.code, -32002);
    const incompatible = await session.request({
      jsonrpc: "2.0",
      id: "editor-incompatible",
      method: "pliego/handshake",
      params: { protocolVersion: "9.0.0" },
    });
    assert.equal(incompatible.error.code, -32602);
    const handshake = await session.request({
      jsonrpc: "2.0",
      id: 1,
      method: "pliego/handshake",
      params: { protocolVersion: "0.2.0-beta.1" },
    });
    assert.equal(handshake.id, 1);
    assert.equal(handshake.result.protocolVersion, "0.2.0-beta.1");
    assert.deepEqual(handshake.result.features, ["diagnostic-links"]);
    assert(handshake.result.methods.includes("pliego/diagnostics"));
    const diagnostics = await session.request({
      jsonrpc: "2.0",
      id: 2,
      method: "pliego/diagnostics",
    });
    assert.equal(diagnostics.result.contract, "dev.pliegors.diagnostics/v1");
    assert.deepEqual(diagnostics.result.diagnostics, []);

    const nullId = await session.request({
      jsonrpc: "2.0",
      id: null,
      method: "pliego/diagnostics",
    });
    assert.equal(nullId.id, null);
    assert.equal(nullId.result.contract, "dev.pliegors.diagnostics/v1");

    const unknown = await session.request({
      jsonrpc: "2.0",
      id: "editor-2",
      method: "editor/private-method",
    });
    assert.equal(unknown.error.code, -32601);
  } finally {
    await session.close();
  }
}

async function checkMcpClient() {
  const session = new Session("mcp");
  try {
    session.notify({ jsonrpc: "2.0", method: "notifications/initialized" });
    const beforeInitialization = await session.request({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/list",
    });
    assert.equal(beforeInitialization.error.code, -32002);

    const rejectedVersion = await session.request({
      jsonrpc: "2.0",
      id: 2,
      method: "initialize",
      params: {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "pliego-reference-client", version: "0.1.0" },
      },
    });
    assert.equal(rejectedVersion.error.code, -32602);
    assert.deepEqual(rejectedVersion.error.data.supported, ["2025-11-25"]);

    const initialized = await session.request({
      jsonrpc: "2.0",
      id: 3,
      method: "initialize",
      params: {
        protocolVersion: "2025-11-25",
        capabilities: {},
        clientInfo: { name: "pliego-reference-client", version: "0.1.0" },
      },
    });
    assert.equal(initialized.result.protocolVersion, "2025-11-25");
    assert.equal(initialized.result.serverInfo.name, "pliegors-opensdk");
    session.notify({ jsonrpc: "2.0", method: "notifications/initialized" });

    const tools = await session.request({ jsonrpc: "2.0", id: 4, method: "tools/list" });
    assert.deepEqual(tools.result.tools.map((tool) => tool.name), ["pliego_sdk_handshake"]);
    const called = await session.request({
      jsonrpc: "2.0",
      id: 5,
      method: "tools/call",
      params: { name: "pliego_sdk_handshake", arguments: {} },
    });
    assert.equal(called.result.isError, false);
    assert.equal(called.result.structuredContent.protocolVersion, "0.2.0-beta.1");
    assert.deepEqual(called.result.structuredContent.features, ["diagnostic-links"]);
  } finally {
    await session.close();
  }
}

class Session {
  constructor(protocol) {
    this.child = spawn(binary, [
      "sdk",
      "tooling-host",
      "--protocol",
      protocol,
      "--feature",
      "diagnostic-links",
    ], {
      cwd: root,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.lines = readline.createInterface({ input: this.child.stdout, crlfDelay: Infinity });
    this.iterator = this.lines[Symbol.asyncIterator]();
    this.stderr = "";
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => { this.stderr += chunk; });
  }

  notify(value) {
    this.child.stdin.write(`${JSON.stringify(value)}\n`);
  }

  async request(value) {
    this.notify(value);
    const result = await Promise.race([
      this.iterator.next(),
      new Promise((_, reject) => setTimeout(
        () => reject(new Error(`tooling response timed out:\n${this.stderr}`)),
        5_000,
      )),
    ]);
    if (result.done) throw new Error(`tooling host closed unexpectedly:\n${this.stderr}`);
    return JSON.parse(result.value);
  }

  async close() {
    this.child.stdin.end();
    const code = await new Promise((resolve, reject) => {
      this.child.once("error", reject);
      this.child.once("exit", resolve);
    });
    this.lines.close();
    assert.equal(code, 0, this.stderr);
  }
}

await checkEditorClient();
await checkMcpClient();
process.stdout.write(
  "OpenSDK tooling PASS: JSON-RPC editor handshake and MCP 2025-11-25 reference client\n",
);
