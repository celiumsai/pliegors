#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { access, mkdir, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { chromium } from "playwright-core";
import { compile } from "svelte/compiler";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixtures = path.join(root, "fixtures", "opensdk", "browser-frameworks");
const output = path.join(root, "target", "opensdk-browser-frameworks");
const assets = path.join(output, "assets");
await rm(output, { recursive: true, force: true });
await mkdir(assets, { recursive: true });

await build({
  entryPoints: {
    react: path.join(fixtures, "react.js"),
    svelte: path.join(fixtures, "svelte.js"),
    lit: path.join(fixtures, "lit.js"),
    adversarial: path.join(fixtures, "adversarial.js"),
  },
  outdir: assets,
  bundle: true,
  format: "esm",
  platform: "browser",
  target: ["es2022"],
  sourcemap: false,
  minify: false,
  legalComments: "none",
  plugins: [{
    name: "svelte",
    setup(esbuild) {
      esbuild.onLoad({ filter: /\.svelte$/ }, async ({ path: file }) => {
        const source = await readFile(file, "utf8");
        const result = compile(source, {
          filename: file,
          generate: "client",
          css: "injected",
          dev: false,
        });
        return { contents: result.js.code, loader: "js" };
      });
    },
  }],
});

const runtime = await readFile(path.join(root, "crates", "pliego-adapters", "src", "runtime-v1.js"));
const html = `<!doctype html><html data-pliego-tier="balanced" data-pliego-motion="reduced"><body>
<script type="module">
globalThis.PliegoAdapters = Object.freeze({ apiVersion: 1, install() {} });
const { createAdapterRuntime } = await import('/runtime-v1.js');
const tick = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));
const definitions = [
  ['react', '/assets/react.js', 'pliego-react-status'],
  ['svelte', '/assets/svelte.js', 'pliego-svelte-status'],
  ['lit', '/assets/lit.js', 'pliego-lit-status'],
  ['adversarial', '/assets/adversarial.js', 'pliego-adversarial-status'],
];

function view(root, tag) {
  const element = root.querySelector(tag);
  const scope = element?.shadowRoot || element;
  const paragraph = scope?.querySelector?.('p');
  return {
    text: paragraph?.textContent || '',
    framework: paragraph?.dataset.framework || '',
    motion: paragraph?.dataset.motion || '',
    status: root.dataset.pliegoStatus || '',
  };
}

async function run() {
  const adapterRuntime = createAdapterRuntime(globalThis);
  adapterRuntime.install();
  const roots = [];
  for (const [framework, module, tag] of definitions) {
    const adapter = document.createElement('pliego-adapter');
    Object.assign(adapter.dataset, {
      pliegoApi: '1',
      pliegoModule: module,
      pliegoProps: JSON.stringify({ message: 'first' }),
      pliegoTrigger: 'immediate',
      pliegoMinTier: 'universal',
      pliegoMotion: 'auto',
      pliegoData: 'auto',
      pliegoCapabilities: 'dom,motion',
    });
    document.body.append(adapter);
    if (!await adapterRuntime.mount(adapter)) throw new Error(framework + ' failed to mount');
    roots.push([framework, adapter, tag]);
  }
  const mounted = Object.fromEntries(roots.map(([name, adapter, tag]) => [name, view(adapter, tag)]));
  for (const [, adapter] of roots) await adapterRuntime.update(adapter, { message: 'second' });
  const updated = Object.fromEntries(roots.map(([name, adapter, tag]) => [name, view(adapter, tag)]));

  document.dispatchEvent(new Event('pliego:adapter-hmr', { bubbles: true, cancelable: true }));
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (roots.every(([, adapter, tag]) => view(adapter, tag).text === 'second' && adapter.dataset.pliegoStatus === 'mounted')) break;
    if (attempt === 199) throw new Error('framework HMR did not settle');
    await tick(10);
  }
  const hmr = Object.fromEntries(roots.map(([name, adapter, tag]) => [name, view(adapter, tag)]));
  adapterRuntime.destroy();
  await tick(50);
  const afterDestroyEvents = globalThis.__pliegoFrameworkMetrics.events.length;
  const afterDestroyTicks = globalThis.__pliegoFrameworkMetrics.resources.ticks;
  document.dispatchEvent(new Event('pliego:adapter-hmr', { bubbles: true, cancelable: true }));
  document.dispatchEvent(new Event('pliego:adversarial-probe'));
  await tick(25);
  return {
    mounted,
    updated,
    hmr,
    disposed: Object.fromEntries(roots.map(([name, adapter]) => [name, {
      children: adapter.childElementCount,
      status: adapter.dataset.pliegoStatus,
    }])),
    active: { ...globalThis.__pliegoFrameworkMetrics.active },
    events: [...globalThis.__pliegoFrameworkMetrics.events],
    eventCountStableAfterDestroy: globalThis.__pliegoFrameworkMetrics.events.length === afterDestroyEvents,
    resourceTicksStableAfterDestroy: globalThis.__pliegoFrameworkMetrics.resources.ticks === afterDestroyTicks,
    resources: { ...globalThis.__pliegoFrameworkMetrics.resources },
  };
}

run().then(
  (value) => { globalThis.__pliegoOpenSdkFrameworkGate = { done: true, value }; },
  (error) => { globalThis.__pliegoOpenSdkFrameworkGate = { done: true, error: String(error?.stack || error) }; },
);
</script></body></html>`;

const server = createServer(async (request, response) => {
  const pathname = new URL(request.url, "http://127.0.0.1").pathname;
  try {
    if (pathname === "/runtime-v1.js") {
      response.writeHead(200, { "content-type": "text/javascript; charset=utf-8" });
      response.end(runtime);
      return;
    }
    if (/^\/assets\/(?:react|svelte|lit|adversarial)\.js$/u.test(pathname)) {
      const bytes = await readFile(path.join(output, pathname.slice(1)));
      response.writeHead(200, { "content-type": "text/javascript; charset=utf-8" });
      response.end(bytes);
      return;
    }
    response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    response.end(html);
  } catch (error) {
    response.writeHead(500, { "content-type": "text/plain; charset=utf-8" });
    response.end(String(error));
  }
});
const port = await listen(server);
const executablePath = await findChrome();
const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--disable-gpu", "--no-sandbox"],
});
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
const browserErrors = [];
page.on("console", (message) => {
  if (message.type() === "error") browserErrors.push(message.text());
});
page.on("pageerror", (error) => browserErrors.push(error.stack || error.message));

try {
  await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: "load" });
  await page.waitForFunction(() => globalThis.__pliegoOpenSdkFrameworkGate?.done, null, {
    timeout: 30_000,
  });
  const gate = await page.evaluate(() => globalThis.__pliegoOpenSdkFrameworkGate);
  assert.equal(gate.error, undefined, gate.error);
  for (const name of ["react", "svelte", "lit", "adversarial"]) {
    assert.deepEqual(gate.value.mounted[name], {
      text: "first",
      framework: name,
      motion: "reduced",
      status: "mounted",
    });
    assert.deepEqual(gate.value.updated[name], {
      text: "second",
      framework: name,
      motion: "reduced",
      status: "mounted",
    });
    assert.deepEqual(gate.value.hmr[name], gate.value.updated[name]);
    assert.deepEqual(gate.value.disposed[name], { children: 0, status: "disposed" });
    assert.equal(gate.value.active[name], 0, `${name} leaked an active framework root`);
    assert(gate.value.events.includes(`${name}:update`), `${name} update was not observed`);
    assert(gate.value.events.filter((event) => event === `${name}:mount`).length >= 2, `${name} HMR did not remount`);
  }
  assert.equal(gate.value.eventCountStableAfterDestroy, true, "runtime listeners survived destroy");
  assert.equal(gate.value.resourceTicksStableAfterDestroy, true, "guest resources remained active after destroy");
  assert.deepEqual(gate.value.resources, {
    timers: 0,
    listeners: 0,
    scopes: 0,
    contexts: 0,
    ticks: gate.value.resources.ticks,
  });
  for (const resource of ["timers", "listeners", "scopes", "contexts"]) {
    assert.equal(gate.value.resources[resource], 0, `${resource} leaked after adversarial dispose`);
  }
  assert.deepEqual(browserErrors, []);
  process.stdout.write(
    `OpenSDK browser frameworks PASS: React 19.2.7, Svelte 5.56.6, Lit 3.3.3; ` +
      `reduced motion, update, HMR, dispose; zero listeners, timers, scopes, contexts\n`,
  );
} finally {
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}

function listen(value) {
  return new Promise((resolve, reject) => {
    value.once("error", reject);
    value.listen(0, "127.0.0.1", () => resolve(value.address().port));
  });
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
