#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-only

import { access, readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const siteRoot = path.join(root, "fixtures", "next", "stress-dashboard", "target", "site");

async function main() {
  const server = createServer((request, response) => serve(request, response));
  const port = await listen(server);
  const browser = await chromium.launch({ executablePath: await findChrome(), headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: "networkidle" });
    await page.waitForSelector("#stress-dashboard", { timeout: 60_000 });
    await expectText(page, "#dataset-size", "1536");
    await expectText(page, "#tick-value", "10");
    await expectText(page, "#event-count", "10");
    await expectText(page, "#visible-count", "1536");
    await expectCount(page, "#dashboard-rows > tr", 1536);
    await expectCount(page, "#dashboard-rows td", 10752);

    const row15 = await page.locator('[data-row-id="15"]').elementHandle();
    const initialChart = await page.getAttribute("#throughput-line", "points");
    await page.selectOption("#region-filter", "west");
    await expectText(page, "#visible-count", "384");
    await page.selectOption("#severity-filter", "critical");
    await expectText(page, "#visible-count", "96");
    if (!(await row15.evaluate((node) => node.isSameNode(document.querySelector('[data-row-id="15"]'))))) {
      throw new Error("retained keyed row lost DOM identity after filters");
    }
    await page.click('[data-row-id="15"]');
    await expectText(page, "#selected-row", "15");
    await page.selectOption("#sort-order", "throughput");
    await page.click("#run-60-updates");
    await expectText(page, "#tick-value", "70");
    await expectText(page, "#event-count", "74");
    await expectContains(page, "#replay-status", "replay parity true");
    if ((await page.getAttribute("#throughput-line", "points")) === initialChart) {
      throw new Error("chart did not change after tick burst");
    }
    if (!(await row15.evaluate((node) => node.isSameNode(document.querySelector('[data-row-id="15"]'))))) {
      throw new Error("retained keyed row lost DOM identity after updates");
    }

    await page.selectOption("#region-filter", "all");
    await page.selectOption("#severity-filter", "all");
    await page.selectOption("#sort-order", "id");
    await expectText(page, "#visible-count", "1536");
    await page.click("#run-60-updates");
    await expectText(page, "#tick-value", "130");
    await expectText(page, "#event-count", "137");

    await page.click("#unmount-dashboard");
    await expectText(page, "#mount-status", "unmounted");
    await expectCount(page, "#dashboard-host > *", 0);
    await row15.evaluate((node) => node.dispatchEvent(new Event("click")));
    const lifecycle = await page.evaluate(async () => {
      const module = await import("/assets/stress_dashboard_next_client.js");
      return JSON.parse(module.run_lifecycle_plateau());
    });
    if (lifecycle.warmupCycles !== 1000 || lifecycle.measuredCycles !== 10000) {
      throw new Error(`lifecycle cycle contract differs: ${JSON.stringify(lifecycle)}`);
    }
    if (!lifecycle.memoryPlateau || lifecycle.domResidue !== 0 || lifecycle.detachedListenerCalls !== 0) {
      throw new Error(`lifecycle cleanup contract failed: ${JSON.stringify(lifecycle)}`);
    }

    await page.click("#mount-dashboard");
    await expectText(page, "#mount-status", "mounted");
    await expectText(page, "#tick-value", "130");
    await expectText(page, "#event-count", "137");
    await expectText(page, "#visible-count", "1536");
    await expectText(page, "#selected-row", "15");
    await expectCount(page, "#dashboard-rows > tr", 1536);
    if (await row15.evaluate((node) => node.isSameNode(document.querySelector('[data-row-id="15"]')))) {
      throw new Error("remounted row unexpectedly reused disposed DOM identity");
    }
    await row15.dispose();
    process.stdout.write(`Next stress dashboard browser PASS: tick 130, events 137, lifecycle ${lifecycle.measuredCycles}\n`);
  } finally {
    await browser.close();
    await close(server);
  }
}

async function serve(request, response) {
  try {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    const relative = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
    const file = path.resolve(siteRoot, ...relative.split("/"));
    const inside = path.relative(siteRoot, file);
    if (!inside || inside.startsWith("..") || path.isAbsolute(inside)) throw new Error("invalid path");
    const details = await stat(file);
    response.writeHead(200, { "Content-Type": contentType(file), "Cache-Control": "no-store" });
    response.end(await readFile(file));
  } catch {
    response.writeHead(404).end("not found");
  }
}

async function expectText(page, selector, value) {
  await page.waitForFunction(({ selector, value }) => document.querySelector(selector)?.textContent === value, { selector, value });
}

async function expectContains(page, selector, value) {
  await page.waitForFunction(({ selector, value }) => document.querySelector(selector)?.textContent?.includes(value), { selector, value });
}

async function expectCount(page, selector, value) {
  await page.waitForFunction(({ selector, value }) => document.querySelectorAll(selector).length === value, { selector, value });
}

function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve(server.address().port));
  });
}

function close(server) {
  return new Promise((resolve) => server.close(resolve));
}

async function findChrome() {
  const candidates = [process.env.CHROME_BIN, "C:/Program Files/Google/Chrome/Application/chrome.exe", "/usr/bin/google-chrome"].filter(Boolean);
  for (const candidate of candidates) {
    try { await access(candidate); return candidate; } catch {}
  }
  throw new Error("Chrome was not found");
}

function contentType(file) {
  if (file.endsWith(".html")) return "text/html; charset=utf-8";
  if (file.endsWith(".js")) return "text/javascript; charset=utf-8";
  if (file.endsWith(".css")) return "text/css; charset=utf-8";
  if (file.endsWith(".wasm")) return "application/wasm";
  return "application/octet-stream";
}

main().catch((error) => {
  process.stderr.write(`${error.stack ?? error}\n`);
  process.exitCode = 1;
});
