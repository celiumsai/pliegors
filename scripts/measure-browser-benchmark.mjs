// SPDX-License-Identifier: Apache-2.0

import { spawn, spawnSync } from 'node:child_process';
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function main() {
  const settings = {
    samples: parseInteger(process.argv[2] ?? '20', 'samples', 1, 100),
    updatesPerSample: parseInteger(process.argv[3] ?? '1000', 'updates per sample', 1, 100_000),
    updateWarmup: parseInteger(process.argv[4] ?? '250', 'update warmup', 0, 100_000),
    plateauBatches: parseInteger(process.argv[5] ?? '6', 'plateau batches', 3, 100),
    plateauBatchCycles: parseInteger(process.argv[6] ?? '500', 'plateau batch cycles', 1, 100_000),
  };
  if (settings.plateauBatches * settings.plateauBatchCycles > 100_000) {
    throw new Error('plateau batches multiplied by cycles must not exceed 100000');
  }
  const output = path.resolve(root, process.argv[7] ?? 'target/benchmarks/p8-browser.json');
  const packageDirectory = path.join(root, 'target', 'benchmarks', 'browser-apply');
  const revision = commandText('git', ['rev-parse', '--verify', 'HEAD'], root);
  const dirty = commandText('git', ['status', '--porcelain', '--untracked-files=normal'], root) !== '';
  if (!/^[0-9a-f]{40}$/u.test(revision)) throw new Error('benchmark source revision is not a full Git commit SHA');
  if (dirty && process.env.PLIEGORS_ALLOW_DIRTY_BENCH !== '1') {
    throw new Error('refusing benchmark evidence from a dirty tree; commit first or use PLIEGORS_ALLOW_DIRTY_BENCH=1 for a non-evidence smoke run');
  }

  const moduleFile = path.join(packageDirectory, 'pliegors_browser_benchmark.js');
  const wasmFile = path.join(packageDirectory, 'pliegors_browser_benchmark_bg.wasm');
  await Promise.all([access(moduleFile), access(wasmFile)]);

  const chrome = await findChrome();
  const profile = await mkdtemp(path.join(os.tmpdir(), 'pliegors-benchmark-chrome-'));
  const flags = [
    '--headless=new',
    '--remote-debugging-port=0',
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-default-apps',
    '--disable-extensions',
    '--disable-sync',
    '--metrics-recording-only',
    'about:blank',
  ];
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url ?? '/', 'http://127.0.0.1');
      if (url.pathname === '/') {
        response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
        response.end(page(settings));
        return;
      }
      const files = new Map([
        ['/pliegors_browser_benchmark.js', [moduleFile, 'text/javascript; charset=utf-8']],
        ['/pliegors_browser_benchmark_bg.wasm', [wasmFile, 'application/wasm']],
      ]);
      const selected = files.get(url.pathname);
      if (!selected) {
        response.writeHead(404).end('not found');
        return;
      }
      response.writeHead(200, { 'Content-Type': selected[1], 'Cache-Control': 'no-store' });
      response.end(await readFile(selected[0]));
    } catch (error) {
      response.writeHead(500).end(String(error));
    }
  });

  let child;
  let cdp;
  try {
    const address = await listen(server);
    child = spawn(chrome, flags, { stdio: 'ignore', windowsHide: true });
    const debuggingPort = await waitForDebuggingPort(profile, child);
    const target = await waitForPageTarget(debuggingPort);
    cdp = await CdpSession.connect(target.webSocketDebuggerUrl);
    await cdp.send('Runtime.enable');
    await cdp.send('Page.enable');
    const browserVersion = await cdp.send('Browser.getVersion');
    await cdp.send('Page.navigate', { url: `http://127.0.0.1:${address.port}/` });
    const raw = await waitForResult(cdp, 60_000);
    const browserEnvironment = await evaluate(cdp, `({
    userAgent: navigator.userAgent,
    hardwareConcurrency: navigator.hardwareConcurrency ?? null,
    deviceMemoryGiB: navigator.deviceMemory ?? null,
    language: navigator.language,
    crossOriginIsolated: globalThis.crossOriginIsolated,
    devicePixelRatio: globalThis.devicePixelRatio
  })`);
    const perUpdate = raw.observations.map((entry) => entry.perUpdateUs);
    const total = raw.observations.map((entry) => entry.totalMs);
    const report = {
      contract: 'dev.pliegors.p8-browser-benchmark/v1',
      revision,
      sourceTreeDirty: dirty,
      createdAt: new Date().toISOString(),
      percentileMethod: 'nearest-rank',
      browser: {
        product: browserVersion.product,
        revision: browserVersion.revision,
        protocolVersion: browserVersion.protocolVersion,
        javascriptVersion: browserVersion.jsVersion,
        executable: chrome,
        launchFlags: flags.filter((flag) => !flag.startsWith('--user-data-dir=')),
      },
      host: {
        platform: process.platform,
        architecture: process.arch,
        osRelease: os.release(),
        cpuModel: os.cpus()[0]?.model?.trim() ?? 'unknown',
        logicalCpuCount: os.cpus().length,
        totalMemoryBytes: os.totalmem(),
        storageClass: process.env.PLIEGORS_BENCH_STORAGE_CLASS ?? 'not declared',
        node: process.version,
        powerAndThermalState: 'not controlled or measured',
      },
      browserEnvironment,
      settings,
      updateSummary: {
        totalMs: summarize(total, 'Ms'),
        perUpdateUs: summarize(perUpdate, 'Us'),
      },
      rawObservations: raw.observations,
      memory: {
        plateauWarmupCycles: raw.plateauWarmupCycles,
        plateauBatchCycles: raw.plateauBatchCycles,
        rawObservations: raw.memoryObservations,
        plateau: raw.memoryPlateau,
        criterion: 'last three WebAssembly.Memory byte lengths equal and zero DOM child residue in every observation',
      },
      limitations: [
        'The synchronous signal loop measures PliegoRS DOM apply work and JavaScript-to-WASM call overhead in this browser.',
        'WebAssembly linear memory is page-granular and may retain freed pages; plateau is not a heap object census.',
        'Headless browser timing applies only to the recorded browser, host, settings, and revision.',
        'No competitor framework is measured or compared by this report.',
      ],
    };
    await mkdir(path.dirname(output), { recursive: true });
    await writeFile(output, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
    process.stdout.write(`browser apply p50/p95: ${report.updateSummary.perUpdateUs.p50Us}/${report.updateSummary.perUpdateUs.p95Us} us\n`);
    process.stdout.write(`memory plateau: ${report.memory.plateau}\n`);
    process.stdout.write(`browser benchmark: ${output}\n`);
  } finally {
    if (cdp) {
      try { await cdp.send('Browser.close'); } catch {}
      cdp.close();
    }
    if (child && child.exitCode === null) child.kill();
    await closeServer(server);
    await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  }
}

function parseInteger(value, label, minimum, maximum) {
  if (!/^[0-9]+$/u.test(value)) throw new Error(`${label} must be an integer`);
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < minimum || number > maximum) {
    throw new Error(`${label} must be between ${minimum} and ${maximum}`);
  }
  return number;
}

function commandText(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed: ${(result.stderr ?? '').trim()}`);
  return (result.stdout ?? '').trim();
}

async function findChrome() {
  const candidates = [
    process.env.CHROME_BIN,
    process.platform === 'win32' ? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe' : undefined,
    process.platform === 'win32' ? 'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe' : undefined,
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {}
  }
  throw new Error('Chrome was not found; set CHROME_BIN to an executable path');
}

function page(values) {
  return `<!doctype html><html><head><meta charset="utf-8"><title>PliegoRS browser benchmark</title></head><body><script type="module">
import init, { run_browser_benchmark } from '/pliegors_browser_benchmark.js';
try {
  await init();
  globalThis.__PLIEGO_BENCH_RESULT__ = JSON.parse(run_browser_benchmark(${values.samples}, ${values.updatesPerSample}, ${values.updateWarmup}, ${values.plateauBatches}, ${values.plateauBatchCycles}));
} catch (error) {
  globalThis.__PLIEGO_BENCH_ERROR__ = String(error?.stack ?? error);
}
</script></body></html>`;
}

function listen(serverInstance) {
  return new Promise((resolve, reject) => {
    serverInstance.once('error', reject);
    serverInstance.listen(0, '127.0.0.1', () => resolve(serverInstance.address()));
  });
}

function closeServer(serverInstance) {
  if (!serverInstance.listening) return Promise.resolve();
  return new Promise((resolve) => serverInstance.close(() => resolve()));
}

async function waitForDebuggingPort(profileDirectory, chromeProcess) {
  const file = path.join(profileDirectory, 'DevToolsActivePort');
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (chromeProcess.exitCode !== null) throw new Error(`Chrome exited before DevTools was available: ${chromeProcess.exitCode}`);
    try {
      const [port] = (await readFile(file, 'utf8')).trim().split(/\r?\n/u);
      if (/^[0-9]+$/u.test(port)) return Number(port);
    } catch {}
    await delay(50);
  }
  throw new Error('timed out waiting for Chrome DevTools port');
}

async function waitForPageTarget(port) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json());
      const pageTarget = targets.find((target) => target.type === 'page' && target.webSocketDebuggerUrl);
      if (pageTarget) return pageTarget;
    } catch {}
    await delay(50);
  }
  throw new Error('timed out waiting for a Chrome page target');
}

async function waitForResult(session, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const state = await evaluate(session, `({result: globalThis.__PLIEGO_BENCH_RESULT__ ?? null, error: globalThis.__PLIEGO_BENCH_ERROR__ ?? null})`);
    if (state.error) throw new Error(`browser benchmark failed: ${state.error}`);
    if (state.result) return state.result;
    await delay(50);
  }
  throw new Error('timed out waiting for browser benchmark result');
}

async function evaluate(session, expression) {
  const response = await session.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (response.exceptionDetails) throw new Error(response.exceptionDetails.text ?? 'browser evaluation failed');
  return response.result.value;
}

function summarize(values, suffix) {
  const ordered = [...values].sort((left, right) => left - right);
  return {
    [`p50${suffix}`]: round(ordered[Math.ceil(ordered.length * 0.50) - 1]),
    [`p95${suffix}`]: round(ordered[Math.ceil(ordered.length * 0.95) - 1]),
  };
}

function round(value) {
  return Math.round(value * 1_000_000) / 1_000_000;
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

class CdpSession {
  static async connect(url) {
    const socket = new WebSocket(url);
    await new Promise((resolve, reject) => {
      socket.addEventListener('open', resolve, { once: true });
      socket.addEventListener('error', () => reject(new Error('cannot connect to Chrome DevTools')), { once: true });
    });
    return new CdpSession(socket);
  }

  constructor(socket) {
    this.socket = socket;
    this.nextId = 1;
    this.pending = new Map();
    socket.addEventListener('message', (event) => {
      const message = JSON.parse(String(event.data));
      if (!message.id) return;
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      if (message.error) pending.reject(new Error(`${pending.method}: ${message.error.message}`));
      else pending.resolve(message.result ?? {});
    });
    socket.addEventListener('close', () => {
      for (const pending of this.pending.values()) pending.reject(new Error(`Chrome DevTools closed during ${pending.method}`));
      this.pending.clear();
    });
  }

  send(method, params = {}) {
    const id = this.nextId;
    this.nextId += 1;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { method, resolve, reject });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  close() {
    this.socket.close();
  }
}

await main();
