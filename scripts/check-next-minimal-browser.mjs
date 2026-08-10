#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-only

import { access, readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export async function checkNextMinimalBrowser({
  siteRoot = path.join(root, "fixtures", "next", "minimal", "target", "site"),
  chrome = process.env.CHROME_BIN,
} = {}) {
  const canonicalSite = path.resolve(siteRoot);
  await access(path.join(canonicalSite, "index.html"));
  const server = createServer((request, response) => serveFixture(canonicalSite, request, response));
  const port = await listen(server);
  const browser = await chromium.launch({
    executablePath: chrome ?? await findChrome(),
    headless: true,
    args: ["--disable-background-networking", "--disable-component-update"],
  });
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: "networkidle" });
    await page.waitForSelector("#causal-app");

    await expectText(page, "#counter-value", "2");
    await expectText(page, "#counter-condition", "counter is even");
    await expectText(page, "#mount-status", "mounted");
    await expectCount(page, "#items li", 2);
    await expectContains(page, "#replay-status", "4 typed events");

    await page.click("#increment");
    await expectText(page, "#counter-value", "3");
    await expectText(page, "#counter-condition", "counter is odd");
    await expectContains(page, "#replay-status", "5 typed events");

    await page.fill("#item-draft", "browser item");
    await expectText(page, "#draft-preview", "browser item");
    await page.click("#add-item");
    await expectCount(page, "#items li", 3);
    await expectContains(page, "#replay-status", "6 typed events");

    const activeEffect = await page.getAttribute("#app-host", "data-owned-effect");
    if (!activeEffect?.startsWith("active:")) throw new Error("owned effect did not run");

    await page.click("#unmount-app");
    await expectText(page, "#mount-status", "unmounted");
    await expectCount(page, "#app-host > *", 0);
    if (await page.getAttribute("#app-host", "data-owned-effect") !== "disposed") {
      throw new Error("owned effect cleanup was not visible after unmount");
    }

    await page.click("#mount-app");
    await expectText(page, "#mount-status", "mounted");
    await expectText(page, "#counter-value", "3");
    await expectCount(page, "#items li", 3);
    await expectText(page, "#draft-preview", "");
    await expectContains(page, "#replay-status", "6 typed events");

    return {
      counter: await page.textContent("#counter-value"),
      items: await page.locator("#items li").count(),
      replay: await page.textContent("#replay-status"),
      mountStatus: await page.textContent("#mount-status"),
    };
  } finally {
    await browser.close();
    await close(server);
  }
}

async function serveFixture(siteRoot, request, response) {
  try {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    const relative = url.pathname === "/" ? "index.html" : decodeURIComponent(url.pathname.slice(1));
    if (!relative || relative.includes("\\") || relative.split("/").includes("..")) {
      response.writeHead(400).end("invalid path");
      return;
    }
    const file = path.resolve(siteRoot, ...relative.split("/"));
    const inside = path.relative(siteRoot, file);
    if (!inside || inside.startsWith("..") || path.isAbsolute(inside)) {
      response.writeHead(400).end("invalid path");
      return;
    }
    const details = await stat(file);
    if (!details.isFile()) throw new Error("not a regular file");
    response.writeHead(200, {
      "Content-Type": contentType(file),
      "Cache-Control": "no-store",
      "Content-Length": details.size,
    });
    response.end(await readFile(file));
  } catch (error) {
    if (error?.code === "ENOENT") response.writeHead(404).end("not found");
    else response.writeHead(500).end("fixture server error");
  }
}

async function expectText(page, selector, expected) {
  await page.waitForFunction(
    ({ selector, expected }) => document.querySelector(selector)?.textContent === expected,
    { selector, expected },
  );
}

async function expectContains(page, selector, expected) {
  await page.waitForFunction(
    ({ selector, expected }) => document.querySelector(selector)?.textContent?.includes(expected),
    { selector, expected },
  );
}

async function expectCount(page, selector, expected) {
  await page.waitForFunction(
    ({ selector, expected }) => document.querySelectorAll(selector).length === expected,
    { selector, expected },
  );
}

function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve(server.address().port));
  });
}

function close(server) {
  if (!server.listening) return Promise.resolve();
  return new Promise((resolve) => server.close(resolve));
}

async function findChrome() {
  const candidates = [
    process.platform === "win32" ? "C:/Program Files/Google/Chrome/Application/chrome.exe" : null,
    process.platform === "win32" ? "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe" : null,
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {}
  }
  throw new Error("Chrome was not found; set CHROME_BIN");
}

function contentType(file) {
  if (file.endsWith(".html")) return "text/html; charset=utf-8";
  if (file.endsWith(".js")) return "text/javascript; charset=utf-8";
  if (file.endsWith(".css")) return "text/css; charset=utf-8";
  if (file.endsWith(".wasm")) return "application/wasm";
  return "application/octet-stream";
}

function parseOptions(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 1) {
    const option = args[index];
    if (option === "--help") return { help: true };
    if (!["--site", "--chrome"].includes(option)) throw new Error(`unknown option: ${option}`);
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
    process.stdout.write("Usage: node scripts/check-next-minimal-browser.mjs [--site <directory>] [--chrome <executable>]\n");
    return;
  }
  const result = await checkNextMinimalBrowser({
    siteRoot: options["--site"] ? path.resolve(options["--site"]) : undefined,
    chrome: options["--chrome"] ? path.resolve(options["--chrome"]) : undefined,
  });
  process.stdout.write(`Next minimal browser PASS: count ${result.counter}, items ${result.items}, ${result.mountStatus}\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error}\n`);
    process.exitCode = 1;
  });
}
