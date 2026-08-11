// SPDX-License-Identifier: AGPL-3.0-only

import { access, cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { chromium } from "playwright-core";
import { nearestRankSummary } from "./next-baseline-lib.mjs";
import { createProcessTreeRssSampler } from "./process-tree-rss.mjs";
import { parseBuildRecord } from "./next-baseline-build.mjs";

const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const MAX_OUTPUT_BYTES = 32 * 1024 * 1024;
const STARTUP_TIMEOUT_MS = 180_000;
const UPDATE_TIMEOUT_MS = 120_000;
const READY_POLL_MS = 100;

export function parseDevUpdate(value, since) {
  assertPlainObject(value, "dev update");
  assertExactKeys(value, ["generation", "kind", "paths", "routes"], "dev update");
  assert(Number.isSafeInteger(value.generation) && value.generation > since, "dev update generation is stale or invalid");
  assert(["none", "css", "content", "adapter", "reload"].includes(value.kind), "dev update kind is invalid");
  validatePathArray(value.paths, "dev update path", true);
  validatePathArray(value.routes, "dev update route", true);
  return structuredClone(value);
}

export function parseSseFrame(source, since) {
  assert(Buffer.byteLength(source) <= 1024 * 1024, "SSE frame exceeds one MiB");
  const normalized = source.replaceAll("\r\n", "\n");
  assert(normalized.endsWith("\n\n"), "SSE frame is unterminated");
  const lines = normalized.slice(0, -2).split("\n");
  if (lines.every((line) => line.startsWith(":"))) return { type: "heartbeat" };
  const eventLines = lines.filter((line) => line.startsWith("event: "));
  const dataLines = lines.filter((line) => line.startsWith("data: "));
  assert(eventLines.length === 1 && eventLines[0] === "event: pliego", "SSE event name differs");
  assert(dataLines.length === 1, "SSE frame must contain one data field");
  assert(lines.length === 2, "SSE frame contains unsupported fields");
  let value;
  try {
    value = JSON.parse(dataLines[0].slice("data: ".length));
  } catch (error) {
    throw new Error(`SSE data is not valid JSON: ${error.message}`);
  }
  return { type: "update", update: parseDevUpdate(value, since) };
}

export function parseRebuildRecord(value, update) {
  assertPlainObject(value, "rebuild record");
  assertExactKeys(value, [
    "recordVersion",
    "generation",
    "changedSources",
    "affectedRoutes",
    "affectedArtifacts",
    "changedArtifacts",
    "hmr",
    "receiptBefore",
    "receiptAfter",
  ], "rebuild record");
  assert(value.recordVersion === "pliego-rebuild/1", "rebuild record version differs");
  assert(value.generation === update.generation, "rebuild record generation differs from dev update");
  validatePathArray(value.changedSources, "changed source", false);
  validatePathArray(value.affectedRoutes, "affected route", true);
  validatePathArray(value.affectedArtifacts, "affected artifact", false);
  validatePathArray(value.changedArtifacts, "changed artifact", false);
  const hmr = parseDevUpdate({ generation: value.generation, ...value.hmr }, 0);
  assert(JSON.stringify(hmr) === JSON.stringify(update), "rebuild HMR differs from dev update");
  assert(value.receiptBefore === null || SHA256_PATTERN.test(value.receiptBefore), "rebuild receiptBefore is invalid");
  assert(SHA256_PATTERN.test(value.receiptAfter), "rebuild receiptAfter is invalid");
  if (value.receiptBefore !== null) assert(value.receiptBefore !== value.receiptAfter, "byte-changing rebuild did not change receipt");
  return structuredClone(value);
}

export function projectFixtureDevMetrics(samples, manifest) {
  assert(samples.length === manifest.policy.measuredRuns, "dev sample count differs from manifest policy");
  for (const [index, sample] of samples.entries()) {
    assert(sample.sample === index + 1, "dev sample sequence differs");
    for (const field of [
      "coldStartMs",
      "warmStartMs",
      "serverUpdateMs",
      "browserVisibleMs",
      "hmrDiscoveryMs",
      "hmrRustWasmMs",
      "hmrSiteMs",
      "hmrVerificationMs",
      "hmrHostTransportMs",
      "domMutations",
    ]) {
      assert(Number.isFinite(sample[field]) && sample[field] >= 0, `dev ${field} is invalid`);
    }
    assert(Number.isSafeInteger(sample.domMutations), "DOM mutation observations must be safe integers");
  }
  const cases = new Map(manifest.measurementCases.map((item) => [item.id, item]));
  const metrics = [
    measured("dev-start-cold", samples, (sample) => sample.coldStartMs, cases),
    measured("dev-start-warm", samples, (sample) => sample.warmStartMs, cases),
    measured("change-to-server", samples, (sample) => sample.serverUpdateMs, cases),
    measured("change-to-browser-visible", samples, (sample) => sample.browserVisibleMs, cases),
    measured("hmr-phase-discovery", samples, (sample) => sample.hmrDiscoveryMs, cases),
    measured("hmr-phase-rust-wasm", samples, (sample) => sample.hmrRustWasmMs, cases),
    measured("hmr-phase-site", samples, (sample) => sample.hmrSiteMs, cases),
    measured("hmr-phase-verification", samples, (sample) => sample.hmrVerificationMs, cases),
    measured("hmr-host-transport-overhead", samples, (sample) => sample.hmrHostTransportMs, cases),
    measured("dom-mutations", samples, (sample) => sample.domMutations, cases),
  ];
  if (samples.every((sample) => Number.isSafeInteger(sample.longSessionRssBytes) && sample.longSessionRssBytes > 0)) {
    metrics.push(measured("long-session-rss", samples, (sample) => sample.longSessionRssBytes, cases));
  }
  return metrics;
}

export function applyFixtureDevMeasurements(report, executions) {
  const projected = structuredClone(report);
  assertUnique(executions.map((execution) => execution.fixtureId), "dev fixture ID");
  const browsers = new Set(executions.map((execution) => execution.browser));
  assert(browsers.size <= 1, "dev fixtures used different browser identities");
  for (const execution of executions) {
    const fixture = projected.fixtures.find((candidate) => candidate.id === execution.fixtureId);
    assert(fixture, `baseline report has no fixture ${execution.fixtureId}`);
    const metrics = new Map(execution.metrics.map((metric) => [metric.caseId, metric]));
    assert(metrics.size === execution.metrics.length, `duplicate dev metric for ${execution.fixtureId}`);
    fixture.results = fixture.results.map((result) => {
      const metric = metrics.get(result.caseId);
      if (metric) return metric;
      if (["remount-count", "full-reload-count"].includes(result.caseId)) {
        return {
          caseId: result.caseId,
          unit: result.unit,
          status: "unavailable",
          reasonCode: "TYPED_HMR_RECEIPT_UNAVAILABLE",
          explanation: "The current dev transport classifies updates but does not emit typed remount or full-reload receipts.",
        };
      }
      if (result.caseId === "long-session-rss" && !metrics.has(result.caseId)) {
        return {
          caseId: result.caseId,
          unit: result.unit,
          status: "unavailable",
          reasonCode: "PLATFORM_COLLECTOR_UNAVAILABLE",
          explanation: "Development process-tree RSS is currently collected only through Linux procfs.",
        };
      }
      return result;
    });
  }
  if (browsers.size === 1) projected.environment.browser = [...browsers][0];
  projected.status = "incomplete";
  projected.bottlenecks = [];
  projected.limitations = [
    ...projected.limitations.filter((item) => !item.startsWith("No development, browser-visible")),
    "CSS HMR measurements include SSE long-poll delivery, stylesheet load, computed-style settlement, and two animation frames.",
    "DOM mutation values count MutationRecord objects, not CSSOM/layout/paint operations.",
    "Typed remount and reload receipts remain unavailable and are never inferred from CSS-only trials.",
  ];
  return projected;
}

export async function collectFixtureDevSamples({ root, fixture, manifest, cli, keep = false }) {
  const fixtureRoot = path.resolve(root, ...fixture.root.split("/"));
  const marker = fixture.id === "minimal" ? "Minimal causal UI" : "Causal operations dashboard";
  const workspaceParent = path.join(root, "target");
  await mkdir(workspaceParent, { recursive: true });
  const runRoots = [];
  const samples = [];
  const totalRuns = manifest.policy.warmupRuns + manifest.policy.measuredRuns;
  let browserVersion = null;
  let activeDev = null;
  let activeBrowser = null;
  try {
    for (let runIndex = 1; runIndex <= totalRuns; runIndex += 1) {
      const sampleRoot = await mkdtemp(path.join(workspaceParent, `next-dev-${fixture.id}-`));
      runRoots.push(sampleRoot);
      const project = path.join(sampleRoot, "application");
      const cargoTarget = path.join(sampleRoot, "cargo-target");
      await cp(fixtureRoot, project, {
        recursive: true,
        filter: (source) => {
          const relative = path.relative(fixtureRoot, source);
          return relative !== "target" && !relative.startsWith(`target${path.sep}`);
        },
      });
      const environment = { CARGO_TARGET_DIR: cargoTarget };
      const coldPort = await availablePort();
      const cold = await startDev({ cli, project, environment, port: coldPort, marker });
      activeDev = cold;
      await cold.stop();
      activeDev = null;

      const warmPort = await availablePort();
      const warm = await startDev({ cli, project, environment, port: warmPort, marker });
      activeDev = warm;
      const browser = await attachBrowser(warmPort, fixture.id);
      activeBrowser = browser;
      browserVersion ??= browser.version;
      const css = path.join(project, "site", "style.css");
      const original = await readFile(css, "utf8");
      const token = `rgb(${17 + runIndex}, ${34 + runIndex}, ${51 + runIndex})`;
      const selector = fixture.id === "minimal" ? "#causal-app" : "#stress-dashboard";
      await browser.arm(selector, token);
      const updatePromise = waitForDevUpdate(warmPort, 0);
      const started = process.hrtime.bigint();
      await writeFile(css, `${original}\n${selector} { outline-color: ${token}; outline-style: solid; }\n`, "utf8");
      const update = await updatePromise;
      const serverUpdateMs = elapsedMs(started);
      assert(update.kind === "css" && update.paths.includes("/assets/style.css"), `${fixture.id} dev update was not CSS`);
      const visible = await browser.waitForVisible(token);
      const browserVisibleMs = elapsedMs(started);
      const longSessionRssBytes = await warm.rssBytes();
      const rebuild = parseRebuildRecord(
        JSON.parse(await readFile(path.join(project, "target", ".pliego", "last-rebuild.json"), "utf8")),
        update,
      );
      const buildRecord = parseBuildRecord(
        JSON.parse(await readFile(path.join(project, "target", ".pliego", "last-build.json"), "utf8")),
      );
      assert(buildRecord.outcome === "executed", `${fixture.id} CSS rebuild did not execute`);
      assert(JSON.stringify(buildRecord.changedSources) === JSON.stringify(["site/style.css"]), `${fixture.id} build record source differs`);
      assert(buildRecord.receiptAfter === rebuild.receiptAfter, `${fixture.id} build and rebuild receipts differ`);
      const recordedMs = buildRecord.phases.totalMicros / 1_000;
      const hostTransportMs = Math.max(0, serverUpdateMs - recordedMs);
      assert(JSON.stringify(rebuild.changedSources) === JSON.stringify(["site/style.css"]), `${fixture.id} rebuild source differs`);
      await browser.close();
      activeBrowser = null;
      await warm.stop();
      activeDev = null;
      const sample = {
        sample: runIndex - manifest.policy.warmupRuns,
        coldStartMs: cold.readyMs,
        warmStartMs: warm.readyMs,
        serverUpdateMs,
        browserVisibleMs,
        hmrDiscoveryMs: buildRecord.phases.discoveryMicros / 1_000,
        hmrRustWasmMs: buildRecord.phases.clientMicros / 1_000,
        hmrSiteMs: buildRecord.phases.siteMicros / 1_000,
        hmrVerificationMs: buildRecord.phases.verificationMicros / 1_000,
        hmrHostTransportMs: hostTransportMs,
        domMutations: visible.mutationRecords,
        longSessionRssBytes,
      };
      if (runIndex > manifest.policy.warmupRuns) samples.push(sample);
    }
    return { samples, browser: browserVersion ?? "not collected" };
  } finally {
    if (activeBrowser) await activeBrowser.close().catch(() => {});
    if (activeDev) await activeDev.stop().catch(() => {});
    if (keep) process.stdout.write(`Next dev workspaces retained:\n${runRoots.join("\n")}\n`);
    else await Promise.all(runRoots.map((directory) => rm(directory, { recursive: true, force: true })));
  }
}

async function startDev({ cli, project, environment, port, marker }) {
  const started = process.hrtime.bigint();
  const child = spawn(cli, ["dev", String(port)], {
    cwd: project,
    env: { ...process.env, ...environment },
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  let closed = null;
  const rss = createProcessTreeRssSampler(child.pid);
  const closedPromise = new Promise((resolve) => {
    child.once("close", (code, signal) => {
      closed = { code, signal };
      resolve(closed);
    });
  });
  child.stdout.on("data", (chunk) => { stdout = bounded(stdout, chunk); });
  child.stderr.on("data", (chunk) => { stderr = bounded(stderr, chunk); });
  const deadline = Date.now() + STARTUP_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (closed) throw new Error(`pliego dev exited early (${closed.signal ?? closed.code})\n${stdout}\n${stderr}`);
    if (stdout.includes("PLIEGO dev: watching ")) {
      try {
        const response = await fetch(`http://127.0.0.1:${port}/`, { signal: AbortSignal.timeout(2_000) });
        const html = await response.text();
        if (response.ok && response.headers.get("cache-control") === "no-store" && html.includes(marker) && html.includes("data-pliego-reload")) {
          return {
            readyMs: elapsedMs(started),
            async rssBytes() { return rss.snapshotBytes(); },
            async stop() {
              await stopProcessTree(child, closedPromise, () => closed !== null);
              await rss.stop();
            },
          };
        }
      } catch {}
    }
    await delay(READY_POLL_MS);
  }
  await stopProcessTree(child, closedPromise, () => closed !== null);
  await rss.stop();
  throw new Error(`pliego dev readiness timed out\n${stdout}\n${stderr}`);
}

async function waitForDevUpdate(port, since) {
  const deadline = Date.now() + UPDATE_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const response = await fetch(`http://127.0.0.1:${port}/_pliego/reload?since=${since}`, {
      signal: AbortSignal.timeout(30_000),
    });
    assert(response.ok, `reload endpoint returned ${response.status}`);
    assert(response.headers.get("content-type")?.startsWith("text/event-stream"), "reload endpoint content type differs");
    const frame = parseSseFrame(await response.text(), since);
    if (frame.type === "update") return frame.update;
  }
  throw new Error("timed out waiting for dev update");
}

async function attachBrowser(port, fixtureId) {
  const browser = await chromium.launch({ executablePath: await findChrome(), headless: true });
  const page = await browser.newPage();
  await page.addInitScript(() => {
    globalThis.__PLIEGO_DEV_MEASURE__ = {
      records: 0,
      expected: null,
      selector: null,
      done: false,
      observer: null,
    };
  });
  await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: "networkidle" });
  await page.waitForSelector(fixtureId === "minimal" ? "#causal-app" : "#stress-dashboard", { timeout: 60_000 });
  return {
    version: await browser.version(),
    async arm(selector, expected) {
      await page.evaluate(({ selector, expected }) => {
        const state = globalThis.__PLIEGO_DEV_MEASURE__;
        state.records = 0;
        state.expected = expected;
        state.selector = selector;
        state.done = false;
        state.observer?.disconnect();
        state.observer = new MutationObserver((records) => { state.records += records.length; });
        state.observer.observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true });
        const settle = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        document.addEventListener("pliego:css-hmr", async () => {
          const deadline = performance.now() + 60_000;
          while (performance.now() < deadline) {
            if (getComputedStyle(document.querySelector(selector)).outlineColor === expected) {
              await settle();
              state.records += state.observer.takeRecords().length;
              state.done = true;
              return;
            }
            await new Promise((resolve) => setTimeout(resolve, 10));
          }
        }, { once: true });
      }, { selector, expected });
    },
    async waitForVisible(expected) {
      await page.waitForFunction((expected) => {
        const state = globalThis.__PLIEGO_DEV_MEASURE__;
        return state.done && state.expected === expected;
      }, expected, { timeout: UPDATE_TIMEOUT_MS });
      return page.evaluate(() => ({ mutationRecords: globalThis.__PLIEGO_DEV_MEASURE__.records }));
    },
    async close() { await browser.close(); },
  };
}

async function stopProcessTree(child, closedPromise, isClosed) {
  if (isClosed()) return;
  if (process.platform === "win32") {
    child.kill();
    await delay(250);
    if (!isClosed()) spawnSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { windowsHide: true });
  } else {
    for (const pid of (await descendantPids(child.pid)).reverse()) {
      try { process.kill(pid, "SIGTERM"); } catch {}
    }
    child.kill("SIGTERM");
    await delay(250);
    if (!isClosed()) {
      for (const pid of (await descendantPids(child.pid)).reverse()) {
        try { process.kill(pid, "SIGKILL"); } catch {}
      }
      child.kill("SIGKILL");
    }
  }
  if (!isClosed()) await closedPromise;
}

async function descendantPids(pid, seen = new Set()) {
  if (!Number.isSafeInteger(pid) || pid <= 0 || seen.has(pid)) return [];
  seen.add(pid);
  let children;
  try {
    children = await readFile(`/proc/${pid}/task/${pid}/children`, "utf8");
  } catch {
    return [];
  }
  const direct = children.trim().split(/\s+/u).filter(Boolean).map(Number);
  const nested = await Promise.all(direct.map((child) => descendantPids(child, seen)));
  return [...direct, ...nested.flat()];
}

function measured(caseId, samples, select, cases) {
  const definition = cases.get(caseId);
  const observations = samples.map((sample) => ({ sample: sample.sample, value: select(sample) }));
  return { caseId, unit: definition.unit, status: "measured", observations, summary: nearestRankSummary(observations.map((item) => item.value)) };
}

function validatePathArray(values, label, leadingSlash) {
  assert(Array.isArray(values), `${label} array is invalid`);
  assertUnique(values, label);
  assert(JSON.stringify(values) === JSON.stringify([...values].sort()), `${label} array is not sorted`);
  for (const value of values) {
    assert(typeof value === "string" && value.length > 0 && !value.includes("\\") && !value.includes("..") && !/[\u0000-\u001f?#]/u.test(value), `${label} is invalid`);
    assert(leadingSlash ? value.startsWith("/") : !value.startsWith("/"), `${label} is invalid`);
  }
}

async function availablePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      server.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

async function findChrome() {
  const candidates = [process.env.CHROME_BIN, "C:/Program Files/Google/Chrome/Application/chrome.exe", "/usr/bin/google-chrome", "/usr/bin/chromium"].filter(Boolean);
  for (const candidate of candidates) {
    try { await access(candidate); return candidate; } catch {}
  }
  throw new Error("Chrome was not found; set CHROME_BIN");
}

function bounded(current, chunk) {
  const next = `${current}${chunk.toString("utf8")}`;
  return next.slice(-MAX_OUTPUT_BYTES);
}

function elapsedMs(started) { return Number(process.hrtime.bigint() - started) / 1_000_000; }
function delay(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }
function assertPlainObject(value, label) { assert(value !== null && typeof value === "object" && !Array.isArray(value), `${label} is not an object`); }
function assertExactKeys(value, expected, label) { assert(JSON.stringify(Object.keys(value).sort()) === JSON.stringify([...expected].sort()), `${label} has missing or unexpected fields`); }
function assertUnique(values, label) { assert(new Set(values).size === values.length, `duplicate ${label}`); }
function assert(condition, message) { if (!condition) throw new Error(message); }
