// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import net from 'node:net';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const runtimeSource = await readFile(
  path.join(root, 'crates/pliego-adapters/src/runtime-v1.js'),
  'utf8',
);
const driverArgument = process.argv.indexOf('--chromedriver');
const chromeDriver = driverArgument >= 0
  ? process.argv[driverArgument + 1]
  : process.env.CHROMEDRIVER;
if (!chromeDriver) throw new Error('set CHROMEDRIVER or pass --chromedriver <path>');

const browserProgram = String.raw`
  globalThis.PliegoAdapters = Object.freeze({ apiVersion: 1, install() {} });
  const { createAdapterRuntime } = await import('/runtime-v1.js');
  const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
  const makeRoot = () => {
    const root = document.createElement('pliego-adapter');
    Object.assign(root.dataset, {
      pliegoApi: '1',
      pliegoModule: '/assets/browser-gate.js',
      pliegoProps: '{}',
      pliegoTrigger: 'immediate',
      pliegoMinTier: 'universal',
      pliegoMotion: 'auto',
      pliegoData: 'auto',
    });
    return root;
  };

  async function run() {
    const scopeCalls = [];
    let releaseMount;
    let scopeSignal;
    const scopeRuntime = createAdapterRuntime(globalThis, async () => ({
      mount(_root, _props, context) {
        scopeSignal = context.signal;
        context.onCleanup(() => scopeCalls.push('registered'));
        return new Promise((resolve) => { releaseMount = resolve; });
      },
      unmount() { scopeCalls.push('unmount'); },
    }));
    scopeRuntime.install();
    const wrapper = document.createElement('section');
    const scopeRoot = makeRoot();
    wrapper.append(scopeRoot);
    document.body.append(wrapper);
    const mounting = scopeRuntime.mount(scopeRoot);
    await tick();
    wrapper.dispatchEvent(new Event('pliego:scope-dispose', { bubbles: true, composed: true }));
    const scopeImmediate = {
      aborted: scopeSignal.aborted,
      connected: scopeRoot.isConnected,
      status: scopeRoot.dataset.pliegoStatus,
      calls: [...scopeCalls],
    };
    releaseMount(() => scopeCalls.push('returned'));
    await mounting;
    await tick();
    const scopeFinal = [...scopeCalls];
    scopeRuntime.destroy();
    wrapper.remove();

    const updateCalls = [];
    let updateSignal;
    const updateRuntime = createAdapterRuntime(globalThis, async () => ({
      mount(_root, _props, context) {
        updateSignal = context.signal;
        context.onCleanup(() => updateCalls.push('registered'));
      },
      update() {
        updateCalls.push('update');
        return new Promise(() => {});
      },
      unmount() { updateCalls.push('unmount'); },
    }));
    updateRuntime.install();
    const updateRoot = makeRoot();
    document.body.append(updateRoot);
    await updateRuntime.mount(updateRoot);
    void updateRuntime.update(updateRoot, { pending: true });
    await tick();
    updateRoot.dispatchEvent(new Event('pliego:scope-dispose', { bubbles: true, composed: true }));
    const updateImmediate = {
      aborted: updateSignal.aborted,
      status: updateRoot.dataset.pliegoStatus,
      calls: [...updateCalls],
    };
    updateRuntime.destroy();
    updateRoot.remove();

    const removalCalls = [];
    const removalRuntime = createAdapterRuntime(globalThis, async () => ({
      mount(_root, _props, context) {
        context.onCleanup(() => removalCalls.push('registered'));
      },
      unmount() { removalCalls.push('unmount'); },
    }));
    removalRuntime.install();
    const removalWrapper = document.createElement('section');
    const removalRoot = makeRoot();
    removalWrapper.append(removalRoot);
    document.body.append(removalWrapper);
    await removalRuntime.mount(removalRoot);
    removalWrapper.remove();
    await tick();
    await tick();
    const removalFinal = {
      status: removalRoot.dataset.pliegoStatus,
      calls: [...removalCalls],
    };
    removalRuntime.destroy();

    return { scopeImmediate, scopeFinal, updateImmediate, removalFinal };
  }

  run().then(
    (value) => { globalThis.__pliegoAdapterGate = { done: true, value }; },
    (error) => {
      globalThis.__pliegoAdapterGate = {
        done: true,
        error: String(error?.stack || error),
      };
    },
  );
`;
const html = `<!doctype html><html><body><script type="module">${browserProgram}</script></body></html>`;

function freePort() {
  return new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.once('error', reject);
    probe.listen(0, '127.0.0.1', () => {
      const { port } = probe.address();
      probe.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

function listen(server) {
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => resolve(server.address().port));
  });
}

async function webdriver(base, endpoint, body) {
  const response = await fetch(`${base}${endpoint}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const value = await response.json().catch(() => ({}));
  if (!response.ok || value?.value?.error) {
    throw new Error(`WebDriver ${endpoint} failed: ${JSON.stringify(value)}`);
  }
  return value.value;
}

const server = createServer((request, response) => {
  if (request.url === '/runtime-v1.js') {
    response.writeHead(200, { 'content-type': 'text/javascript; charset=utf-8' });
    response.end(runtimeSource);
    return;
  }
  response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
  response.end(html);
});
const sitePort = await listen(server);
const driverPort = await freePort();
const driver = spawn(chromeDriver, [`--port=${driverPort}`, '--allowed-ips=127.0.0.1'], {
  stdio: ['ignore', 'pipe', 'pipe'],
  windowsHide: true,
});
let driverLog = '';
driver.stdout.on('data', (chunk) => { driverLog += chunk; });
driver.stderr.on('data', (chunk) => { driverLog += chunk; });
const base = `http://127.0.0.1:${driverPort}`;
let sessionId;

try {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      await webdriver(base, '/status');
      break;
    } catch (error) {
      if (attempt === 99) throw new Error(`${error.message}\n${driverLog}`);
      await delay(50);
    }
  }
  const session = await webdriver(base, '/session', {
    capabilities: {
      alwaysMatch: {
        browserName: 'chrome',
        'goog:chromeOptions': {
          args: ['--headless=new', '--disable-gpu', '--no-sandbox', '--window-size=1280,900'],
        },
      },
    },
  });
  sessionId = session.sessionId;
  await webdriver(base, `/session/${sessionId}/url`, { url: `http://127.0.0.1:${sitePort}/` });

  let gate;
  for (let attempt = 0; attempt < 200; attempt += 1) {
    gate = await webdriver(base, `/session/${sessionId}/execute/sync`, {
      script: 'return globalThis.__pliegoAdapterGate || null;',
      args: [],
    });
    if (gate?.done) break;
    await delay(25);
  }
  assert.equal(gate?.done, true, 'browser adapter gate timed out');
  assert.equal(gate.error, undefined, gate.error);
  assert.deepEqual(gate.value.scopeImmediate, {
    aborted: true,
    connected: true,
    status: 'disposed',
    calls: ['unmount', 'registered'],
  });
  assert.deepEqual(gate.value.scopeFinal, ['unmount', 'registered', 'returned']);
  assert.deepEqual(gate.value.updateImmediate, {
    aborted: true,
    status: 'disposed',
    calls: ['update', 'unmount', 'registered'],
  });
  assert.deepEqual(gate.value.removalFinal, {
    status: 'disposed',
    calls: ['unmount', 'registered'],
  });
  console.log('Adapter browser lifecycle PASS: scope, pending update, and DOM removal');
} finally {
  if (sessionId) {
    await fetch(`${base}/session/${sessionId}`, { method: 'DELETE' }).catch(() => {});
  }
  driver.kill();
  await new Promise((resolve) => server.close(resolve));
}
