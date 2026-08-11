#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-only

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { access, mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const contract = "dev.pliegors.hyphae-console-acceptance/v1";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifest = path.join(root, "fixtures", "next", "hyphae-console", "Cargo.toml");
const executable = process.env.HYPHAE_V101_BIN;
assert(executable, "HYPHAE_V101_BIN is required");

class AcceptanceServer {
  static async start(dataDirectory) {
    const child = spawn(acceptanceServerPath(), [], {
      cwd: root,
      env: {
        ...process.env,
        HYPHAE_V101_BIN: executable,
        PLIEGO_HYPHAE_DATA_DIR: dataDirectory,
        PLIEGO_HYPHAE_PARENT_PID: String(process.pid),
      },
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
      detached: process.platform !== "win32",
    });
    const server = new AcceptanceServer(child);
    try {
      await server.waitFor("ready");
      return server;
    } catch (error) {
      await server.forceStop();
      throw error;
    }
  }

  constructor(child) {
    this.child = child;
    this.stderr = "";
    this.records = [];
    this.waiters = [];
    this.recordError = undefined;
    this.childError = undefined;
    this.lines = readline.createInterface({ input: child.stdout, crlfDelay: Infinity });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => { this.stderr += chunk; });
    child.on("error", (error) => {
      this.childError = error;
      this.wakeWaiters();
    });
    this.lines.on("line", (line) => this.acceptRecord(line));
  }

  get origin() {
    return this.ready.origin;
  }

  async stop() {
    this.child.stdin.end("shutdown\n");
    const stopped = await this.waitFor("stopped");
    const code = await waitForExit(this.child, this.stderr);
    assert.equal(code, 0, this.stderr);
    this.lines.close();
    return { ...this.ready, stopped };
  }

  async forceStop() {
    if (this.child.exitCode === null && this.child.stdin.writable) {
      this.child.stdin.end("shutdown\n");
    }
    await waitForExit(this.child, this.stderr).catch(() => undefined);
    this.lines.close();
  }

  acceptRecord(line) {
    let record;
    try {
      record = JSON.parse(line);
    } catch {
      this.stderr += `\ninvalid stdout: ${line}`;
      return;
    }
    if (record.contract !== contract) {
      this.recordError = new Error(`acceptance record contract differs: ${record.contract}`);
      this.wakeWaiters();
      return;
    }
    this.records.push(record);
    this.wakeWaiters();
  }

  wakeWaiters() {
    const waiters = this.waiters.splice(0);
    for (const wake of waiters) wake();
  }

  async waitFor(event) {
    const deadline = Date.now() + 45_000;
    while (Date.now() < deadline) {
      const record = this.records.find((candidate) => candidate.event === event);
      if (record) {
        if (event === "ready") this.ready = record;
        return record;
      }
      if (this.recordError) throw this.recordError;
      if (this.childError) throw this.childError;
      if (this.child.exitCode !== null) {
        throw new Error(`acceptance server exited (${this.child.exitCode}):\n${this.stderr}`);
      }
      await Promise.race([
        new Promise((resolve) => this.waiters.push(resolve)),
        delay(100),
      ]);
    }
    throw new Error(`acceptance server timed out waiting for ${event}:\n${this.stderr}`);
  }
}

async function login(origin, username, javaScriptEnabled) {
  const context = await browser.newContext({ javaScriptEnabled });
  const page = await context.newPage();
  const errors = [];
  const requests = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.stack || error.message));
  page.on("request", (request) => requests.push(request.url()));
  const form = await page.goto(`${origin}/login`, { waitUntil: "load" });
  assert.equal(form?.status(), 200);
  assert.equal(await page.locator("script").count(), 0);
  await page.locator('input[name="username"]').fill(username);
  await page.locator('input[name="password"]').fill("preview-only");
  const response = await context.request.post(`${origin}/login`, {
    headers: formHeaders(origin),
    data: new URLSearchParams({
      username,
      password: "preview-only",
      _csrf: await page.locator('input[name="_csrf"]').inputValue(),
    }).toString(),
    maxRedirects: 0,
  });
  assert.equal(response.status(), 303, await response.text());
  await page.goto(`${origin}/console`, { waitUntil: "load" });
  context.acceptance = { page, errors, requests, username };
  return context;
}

async function increment(context, origin, count) {
  const { page } = context.acceptance;
  for (let index = 0; index < count; index += 1) {
    const response = await context.request.post(`${origin}/console/increment`, {
      headers: formHeaders(origin),
      data: new URLSearchParams({
        expected_revision: await page.locator('input[name="expected_revision"]').inputValue(),
        _csrf: await page.locator('input[name="_csrf"]').inputValue(),
      }).toString(),
      maxRedirects: 0,
    });
    assert.equal(response.status(), 303, await response.text());
    await page.reload({ waitUntil: "load" });
  }
}

async function assertTenantIsolation(alice, bob, origin, aliceCount, bobCount) {
  const aliceHtml = await consoleHtml(alice, origin);
  const bobHtml = await consoleHtml(bob, origin);
  assert.match(aliceHtml, new RegExp(`Counter ${aliceCount}`, "u"));
  assert.doesNotMatch(aliceHtml, /tenant-b/u);
  assert.match(bobHtml, new RegExp(`Counter ${bobCount}`, "u"));
  assert.doesNotMatch(bobHtml, /tenant-a/u);
  await assertBrowserBoundary(alice, origin, aliceCount, aliceHtml);
  await assertBrowserBoundary(bob, origin, bobCount, bobHtml);
}

async function assertBrowserBoundary(context, origin, activityCount, html) {
  const { page, errors, requests } = context.acceptance;
  const activity = await page.goto(`${origin}/console/activity`, { waitUntil: "load" });
  assert.equal(activity?.status(), 200);
  assert.equal(await page.locator("article").count(), activityCount);
  const raw = await context.request.get(`${origin}/v2/capabilities`);
  assert.equal(raw.status(), 404);
  for (const request of requests) {
    assert.equal(new URL(request).origin, origin);
    assert.doesNotMatch(request, /\/v2\/(?:execute|capabilities)/u);
  }
  for (const forbidden of [
    "application/vnd.hyphae.product-v1",
    "application/vnd.hyphae.error-v1",
    "84161cf067141b60f4847b965ef77c5b749749c0",
  ]) {
    assert.doesNotMatch(html, new RegExp(forbidden, "u"));
  }
  assert.deepEqual(errors, []);
}

async function consoleHtml(context, origin) {
  const { page } = context.acceptance;
  await page.goto(`${origin}/console`, { waitUntil: "load" });
  return page.content();
}

async function sessionCookie(context) {
  const cookies = await context.cookies();
  const cookie = cookies.find((candidate) => candidate.name === "pliego-hyphae-acceptance-session");
  assert(cookie, "acceptance session cookie is missing");
  return cookie;
}

async function assertReleased(lifecycle) {
  assert.equal(lifecycle.appPid, lifecycle.stopped.appPid);
  assert.equal(lifecycle.sidecarPid, lifecycle.stopped.sidecarPid);
  assert.equal(isProcessAlive(lifecycle.appPid), false, "application process remains alive");
  assert.equal(isProcessAlive(lifecycle.sidecarPid), false, "Hyphae sidecar process remains alive");
  await assertEndpointRefused({ port: Number(new URL(lifecycle.origin).port) });
  await assertEndpointRefused({ port: Number(lifecycle.sidecarHttpAddress.split(":").at(-1)) });
  const nativePath = process.platform === "win32"
    ? `\\\\.\\pipe\\${lifecycle.nativeEndpoint}`
    : lifecycle.nativeEndpoint;
  await assertEndpointRefused({ path: nativePath });
}

async function assertEndpointRefused(endpoint) {
  await new Promise((resolve, reject) => {
    const label = endpoint.path ?? `127.0.0.1:${endpoint.port}`;
    const socket = net.connect(endpoint.path ? endpoint : { host: "127.0.0.1", ...endpoint });
    socket.setTimeout(1_000);
    socket.once("connect", () => {
      socket.destroy();
      reject(new Error(`${label} remained reachable after shutdown`));
    });
    socket.once("error", (error) => {
      const expected = endpoint.path
        ? ["ENOENT", "ECONNREFUSED", "ENXIO"]
        : ["ECONNREFUSED"];
      if (expected.includes(error.code)) {
        resolve();
      } else {
        reject(new Error(`${label} failed unexpectedly: ${error.code} ${error.message}`));
      }
    });
    socket.once("timeout", () => {
      socket.destroy();
      reject(new Error(`${label} did not refuse connections`));
    });
  });
}

function buildAcceptanceServer() {
  assert.equal(process.env.CARGO_BUILD_TARGET, undefined, "CARGO_BUILD_TARGET is unsupported");
  const result = spawnSync("cargo", [
    "build",
    "--manifest-path", manifest,
    "--package", "pliegors-next-hyphae-console-server",
    "--example", "hyphae-console-acceptance-server",
    "--features", "acceptance-harness",
    "--locked",
  ], { cwd: root, encoding: "utf8", windowsHide: true });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
}

function acceptanceServerPath() {
  const targetDirectory = process.env.CARGO_TARGET_DIR
    ? path.resolve(root, process.env.CARGO_TARGET_DIR)
    : path.join(root, "fixtures", "next", "hyphae-console", "target");
  return path.join(
    targetDirectory,
    "debug", "examples",
    process.platform === "win32"
      ? "hyphae-console-acceptance-server.exe"
      : "hyphae-console-acceptance-server",
  );
}

function formHeaders(origin) {
  return {
    origin,
    "content-type": "application/x-www-form-urlencoded",
  };
}

function isProcessAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}

function waitForExit(child, stderr) {
  if (child.exitCode !== null) return Promise.resolve(child.exitCode);
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      cleanup();
      terminateProcessTree(child.pid);
      reject(new Error(`acceptance server did not exit:\n${stderr}`));
    }, 15_000);
    const exited = (code) => {
      cleanup();
      resolve(code);
    };
    const failed = (error) => {
      cleanup();
      reject(error);
    };
    const cleanup = () => {
      clearTimeout(timeout);
      child.off("exit", exited);
      child.off("error", failed);
    };
    child.once("exit", exited);
    child.once("error", failed);
  });
}

function terminateProcessTree(pid) {
  if (process.platform === "win32") {
    spawnSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], {
      stdio: "ignore",
      windowsHide: true,
    });
    return;
  }
  try {
    process.kill(-pid, "SIGKILL");
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function findChrome() {
  const candidates = [
    process.env.CHROME,
    process.env.CHROME_PATH,
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
    "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {}
  }
  throw new Error("Chrome or Chromium was not found; set CHROME_PATH");
}

let temporaryRoot;
let browser;
let server;
try {
  temporaryRoot = await mkdtemp(path.join(os.tmpdir(), "pliego-hyphae-"));
  const dataDirectory = path.join(temporaryRoot, "data");
  browser = await chromium.launch({
    executablePath: await findChrome(),
    headless: true,
    args: ["--disable-gpu", "--no-sandbox"],
  });
  buildAcceptanceServer();
  server = await AcceptanceServer.start(dataDirectory);
  const alice = await login(server.origin, "alice", true);
  const aliceCookie = await sessionCookie(alice);
  await increment(alice, server.origin, 2);
  assert.match(await consoleHtml(alice, server.origin), /Counter 2/u);

  const bob = await login(server.origin, "bob", false);
  await increment(bob, server.origin, 1);
  await assertTenantIsolation(alice, bob, server.origin, 2, 1);
  await alice.close();
  await bob.close();

  const firstProcess = await server.stop();
  server = undefined;
  await assertReleased(firstProcess);
  server = await AcceptanceServer.start(dataDirectory);
  const stale = await browser.newContext({ javaScriptEnabled: false });
  await stale.addCookies([{
    name: aliceCookie.name,
    value: aliceCookie.value,
    url: server.origin,
  }]);
  assert.equal(
    (await stale.cookies(server.origin)).some(
      (cookie) => cookie.name === aliceCookie.name && cookie.value === aliceCookie.value,
    ),
    true,
    "stale acceptance cookie was not installed for the restarted origin",
  );
  const staleResponse = await stale.request.get(`${server.origin}/console`);
  assert.equal(staleResponse.status(), 401, "an in-memory session survived process restart");
  await stale.close();

  const restartedAlice = await login(server.origin, "alice", false);
  const restartedBob = await login(server.origin, "bob", true);
  await assertTenantIsolation(restartedAlice, restartedBob, server.origin, 2, 1);
  await restartedAlice.close();
  await restartedBob.close();

  const secondProcess = await server.stop();
  server = undefined;
  await assertReleased(secondProcess);
  process.stdout.write(
    `Hyphae Console full-process acceptance PASS: ${secondProcess.origin}\n`,
  );
} finally {
  if (server) {
    await server.stop().catch(async () => server.forceStop());
  }
  if (browser) await browser.close().catch(() => undefined);
  if (temporaryRoot) await rm(temporaryRoot, { recursive: true, force: true });
}
