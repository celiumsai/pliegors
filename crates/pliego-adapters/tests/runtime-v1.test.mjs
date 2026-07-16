// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import test from 'node:test';
import { API_VERSION, createAdapterRuntime } from '../src/runtime-v1.js';

class FakeEvent {
  constructor(type, options = {}) {
    this.type = type;
    this.detail = options.detail;
    this.target = options.target;
  }
}

class FakeTarget {
  constructor() {
    this.listeners = new Map();
  }

  addEventListener(name, callback) {
    const listeners = this.listeners.get(name) || new Set();
    listeners.add(callback);
    this.listeners.set(name, listeners);
  }

  removeEventListener(name, callback) {
    this.listeners.get(name)?.delete(callback);
  }

  emit(name, detail = {}, target = this) {
    for (const callback of [...(this.listeners.get(name) || [])]) {
      callback(new FakeEvent(name, { detail, target }));
    }
  }
}

class FakeMediaQuery extends FakeTarget {
  constructor(matches) {
    super();
    this.matches = matches;
  }

  set(matches) {
    this.matches = matches;
    this.emit('change', { matches });
  }
}

class FakeConnection extends FakeTarget {
  constructor(saveData) {
    super();
    this.saveData = saveData;
  }

  setSaveData(saveData) {
    this.saveData = saveData;
    this.emit('change', { saveData });
  }
}

class FakeRoot {
  constructor(overrides = {}) {
    this.dataset = {
      pliegoApi: '1',
      pliegoModule: '/assets/plugin.1234.js',
      pliegoProps: '{}',
      pliegoTrigger: 'immediate',
      pliegoMinTier: 'universal',
      pliegoMotion: 'auto',
      pliegoData: 'auto',
      ...overrides,
    };
    this.events = [];
    this.isConnected = true;
    this.listeners = new Map();
  }

  dispatchEvent(event) {
    this.events.push(event);
    return true;
  }

  addEventListener(name, callback) {
    const listeners = this.listeners.get(name) || new Set();
    listeners.add(callback);
    this.listeners.set(name, listeners);
  }

  removeEventListener(name, callback) {
    this.listeners.get(name)?.delete(callback);
  }

  fire(name) {
    for (const callback of [...(this.listeners.get(name) || [])]) {
      callback(new FakeEvent(name, { target: this }));
    }
  }

  matches(selector) {
    return selector === 'pliego-adapter';
  }

  querySelectorAll() {
    return [];
  }
}

function environment({ saveData = false, reduced = false, tier = 'balanced' } = {}) {
  const errors = [];
  const roots = [];
  const media = new FakeMediaQuery(reduced);
  const connection = new FakeConnection(saveData);
  const windowEvents = new FakeTarget();
  const documentEvents = new FakeTarget();
  return {
    errors,
    roots,
    media,
    connection,
    location: new URL('https://pliego.test/'),
    navigator: { connection },
    matchMedia: () => media,
    CustomEvent: FakeEvent,
    console: { error: (...args) => errors.push(args) },
    document: {
      documentElement: { dataset: { pliegoTier: tier } },
      querySelectorAll: () => roots,
      addEventListener: documentEvents.addEventListener.bind(documentEvents),
      removeEventListener: documentEvents.removeEventListener.bind(documentEvents),
      emit: documentEvents.emit.bind(documentEvents),
    },
    setTimeout,
    clearTimeout,
    queueMicrotask,
    addEventListener: windowEvents.addEventListener.bind(windowEvents),
    removeEventListener: windowEvents.removeEventListener.bind(windowEvents),
    emit: windowEvents.emit.bind(windowEvents),
  };
}

const nextTurn = () => new Promise((resolve) => setTimeout(resolve, 0));

test('v1 executes mount, update and automatic cleanup in a stable order', async () => {
  const env = environment();
  const root = new FakeRoot({ pliegoProps: '{"step":1}' });
  const calls = [];
  const plugin = {
    apiVersion: API_VERSION,
    mount(_root, props, context) {
      calls.push(`mount:${props.step}:${context.tier}:${context.motion}`);
      context.onCleanup(() => calls.push('registered:first'));
      context.onCleanup(() => calls.push('registered:last'));
      return () => calls.push('returned');
    },
    update(_root, props, context) {
      calls.push(`update:${props.step}:${context.apiVersion}`);
    },
    unmount() {
      calls.push('unmount');
    },
  };
  const runtime = createAdapterRuntime(env, async () => ({ pliegoAdapter: plugin }));

  assert.equal(await runtime.mount(root), true);
  assert.equal(await runtime.update(root, { step: 2 }), true);
  assert.equal(runtime.unmount(root, 'test'), true);

  assert.deepEqual(calls, [
    'mount:1:balanced:full',
    'update:2:1',
    'unmount',
    'returned',
    'registered:last',
    'registered:first',
  ]);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
  assert.deepEqual(root.events.map((event) => event.type), [
    'pliego:mount',
    'pliego:update',
    'pliego:unmount',
  ]);
});

test('legacy mount-only modules remain compatible and remount on update', async () => {
  const env = environment();
  const root = new FakeRoot();
  let mounts = 0;
  let cleanups = 0;
  const runtime = createAdapterRuntime(env, async () => ({
    mount() {
      mounts += 1;
      return () => { cleanups += 1; };
    },
  }));

  await runtime.mount(root);
  await runtime.update(root, { changed: true });

  assert.equal(mounts, 2);
  assert.equal(cleanups, 1);
  assert.equal(root.dataset.pliegoProps, '{"changed":true}');
});

test('save-data, reduced-motion and capability tiers deny import before execution', async () => {
  for (const scenario of [
    {
      env: environment({ saveData: true }),
      root: new FakeRoot({ pliegoData: 'skip' }),
      reason: 'save-data',
    },
    {
      env: environment({ reduced: true }),
      root: new FakeRoot({ pliegoMotion: 'skip' }),
      reason: 'reduced-motion',
    },
    {
      env: environment({ tier: 'universal' }),
      root: new FakeRoot({ pliegoCapabilities: 'webgl' }),
      reason: 'tier',
    },
  ]) {
    let imports = 0;
    const runtime = createAdapterRuntime(scenario.env, async () => {
      imports += 1;
      return { mount() {} };
    });
    assert.equal(await runtime.mount(scenario.root), false);
    assert.equal(imports, 0);
    assert.equal(scenario.root.dataset.pliegoStatus, 'skipped');
    assert.equal(scenario.root.dataset.pliegoSkipReason, scenario.reason);
  }
});

test('tampered paths, versions and capabilities fail without poisoning another island', async () => {
  const env = environment();
  const invalid = [
    new FakeRoot({ pliegoModule: 'https://evil.test/plugin.js' }),
    new FakeRoot({ pliegoModule: '/assets/%2e%2e/secret.js' }),
    new FakeRoot({ pliegoApi: '2' }),
    new FakeRoot({ pliegoCapabilities: 'shell-access' }),
  ];
  let mounts = 0;
  const runtime = createAdapterRuntime(env, async () => ({
    apiVersion: 1,
    mount() { mounts += 1; },
  }));

  for (const root of invalid) {
    assert.equal(await runtime.mount(root), false);
    assert.equal(root.dataset.pliegoStatus, 'error');
    assert.ok(root.events.some((event) => event.type === 'pliego:error'));
  }
  const healthy = new FakeRoot();
  assert.equal(await runtime.mount(healthy), true);
  assert.equal(mounts, 1);
  assert.equal(healthy.dataset.pliegoStatus, 'mounted');
});

test('file origins cannot turn root-relative modules into local file reads', async () => {
  const env = environment();
  env.location = new URL('file:///tmp/site/index.html');
  let imports = 0;
  const runtime = createAdapterRuntime(env, async () => {
    imports += 1;
    return { mount() {} };
  });
  const root = new FakeRoot();

  assert.equal(await runtime.mount(root), false);
  assert.equal(imports, 0);
  assert.equal(root.dataset.pliegoStatus, 'error');
});

test('removal during a pending import prevents a late mount', async () => {
  const env = environment();
  const root = new FakeRoot();
  let release;
  let mounts = 0;
  const runtime = createAdapterRuntime(env, () => new Promise((resolve) => { release = resolve; }));

  const pending = runtime.mount(root);
  runtime.unmount(root, 'dom-removal');
  root.isConnected = false;
  release({ mount() { mounts += 1; } });

  assert.equal(await pending, false);
  assert.equal(mounts, 0);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
});

test('cleanup failures are isolated and later cleanup callbacks still run', async () => {
  const env = environment();
  const root = new FakeRoot();
  const calls = [];
  const runtime = createAdapterRuntime(env, async () => ({
    mount(_root, _props, context) {
      context.onCleanup(() => { calls.push('registered'); });
      return () => { calls.push('returned'); throw new Error('return failure'); };
    },
    unmount() {
      calls.push('unmount');
      throw new Error('unmount failure');
    },
  }));

  await runtime.mount(root);
  runtime.unmount(root);

  assert.deepEqual(calls, ['unmount', 'returned', 'registered']);
  assert.equal(root.events.filter((event) => event.type === 'pliego:cleanup-error').length, 2);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
});

test('interaction trigger imports only after the first user intent', async () => {
  const env = environment();
  const root = new FakeRoot({ pliegoTrigger: 'interaction' });
  let imports = 0;
  const runtime = createAdapterRuntime(env, async () => {
    imports += 1;
    return { mount() {} };
  });

  runtime.schedule(root);
  assert.equal(imports, 0);
  assert.equal(root.dataset.pliegoStatus, 'scheduled');
  root.fire('focusin');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(imports, 1);
  assert.equal(root.dataset.pliegoStatus, 'mounted');
});

test('a late update cannot revive an island after unmount', async () => {
  const env = environment();
  const root = new FakeRoot();
  let release;
  const runtime = createAdapterRuntime(env, async () => ({
    mount() {},
    update() {
      return new Promise((resolve) => { release = resolve; });
    },
  }));

  await runtime.mount(root);
  const pending = runtime.update(root, { step: 2 });
  runtime.unmount(root, 'route-change');
  release();

  assert.equal(await pending, false);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
  assert.equal(root.events.some((event) => event.type === 'pliego:update'), false);
});

test('unknown policies, triggers and oversized updates fail closed', async () => {
  const env = environment();
  let imports = 0;
  const runtime = createAdapterRuntime(env, async () => {
    imports += 1;
    return { mount() {}, update() {} };
  });
  const badPolicy = new FakeRoot({ pliegoMotion: 'invented' });
  assert.equal(await runtime.mount(badPolicy), false);
  assert.equal(imports, 0);
  const badTrigger = new FakeRoot({ pliegoTrigger: 'eagerly' });
  runtime.schedule(badTrigger);
  assert.equal(badTrigger.dataset.pliegoStatus, 'error');
  assert.equal(imports, 0);
  const healthy = new FakeRoot();
  await runtime.mount(healthy);
  assert.equal(await runtime.update(healthy, { payload: 'x'.repeat(32768) }), false);
  assert.equal(healthy.dataset.pliegoStatus, 'error');
});

test('props limits count UTF-8 bytes rather than JavaScript code units', async () => {
  const env = environment();
  let imports = 0;
  const runtime = createAdapterRuntime(env, async () => {
    imports += 1;
    return { mount() {}, update() {} };
  });
  const unicodePayload = { payload: '\u{1F642}'.repeat(9000) };
  const serialized = JSON.stringify(unicodePayload);
  assert.ok(serialized.length < 32768);
  assert.ok(new TextEncoder().encode(serialized).length > 32768);

  const oversizedRoot = new FakeRoot({ pliegoProps: serialized });
  assert.equal(await runtime.mount(oversizedRoot), false);
  assert.equal(oversizedRoot.dataset.pliegoStatus, 'error');
  assert.equal(imports, 0);

  const healthy = new FakeRoot();
  assert.equal(await runtime.mount(healthy), true);
  assert.equal(await runtime.update(healthy, unicodePayload), false);
  assert.equal(healthy.dataset.pliegoStatus, 'error');
});

test('an aborted mount generation cannot delete or revive its successor', async () => {
  const env = environment();
  const root = new FakeRoot();
  const releases = [];
  let mounts = 0;
  const runtime = createAdapterRuntime(env, () => new Promise((resolve) => releases.push(resolve)));

  const first = runtime.mount(root);
  runtime.refresh(root);
  releases[0]({ mount() { mounts += 1; } });
  assert.equal(await first, false);
  await nextTurn();
  assert.equal(releases.length, 2);
  runtime.unmount(root, 'route-change');
  releases[1]({ mount() { mounts += 1; } });
  await nextTurn();

  assert.equal(mounts, 0);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
});

test('updates are serialized and preserve invocation order', async () => {
  const env = environment();
  const root = new FakeRoot();
  const calls = [];
  const releases = new Map();
  const runtime = createAdapterRuntime(env, async () => ({
    mount() {},
    update(_root, props) {
      calls.push(`start:${props.step}`);
      return new Promise((resolve) => releases.set(props.step, () => {
        calls.push(`end:${props.step}`);
        resolve();
      }));
    },
  }));

  await runtime.mount(root);
  const first = runtime.update(root, { step: 1 });
  const second = runtime.update(root, { step: 2 });
  assert.deepEqual(calls, ['start:1']);
  releases.get(1)();
  await nextTurn();
  assert.deepEqual(calls, ['start:1', 'end:1', 'start:2']);
  releases.get(2)();
  assert.deepEqual(await Promise.all([first, second]), [true, true]);
  assert.equal(root.dataset.pliegoProps, '{"step":2}');
});

test('an update received during import is applied after mount', async () => {
  const env = environment();
  const root = new FakeRoot({ pliegoProps: '{"step":0}' });
  let releaseImport;
  const calls = [];
  const runtime = createAdapterRuntime(env, () => new Promise((resolve) => { releaseImport = resolve; }));

  const mounting = runtime.mount(root);
  const updating = runtime.update(root, { step: 3 });
  releaseImport({
    mount(_root, props) { calls.push(`mount:${props.step}`); },
    update(_root, props) { calls.push(`update:${props.step}`); },
  });

  assert.equal(await mounting, true);
  assert.equal(await updating, true);
  assert.deepEqual(calls, ['mount:0', 'update:3']);
});

test('a rejected update after unmount remains disposed and silent', async () => {
  const env = environment();
  const root = new FakeRoot();
  let rejectUpdate;
  const runtime = createAdapterRuntime(env, async () => ({
    mount() {},
    update() { return new Promise((_, reject) => { rejectUpdate = reject; }); },
  }));

  await runtime.mount(root);
  const updating = runtime.update(root, { step: 1 });
  runtime.unmount(root, 'route-change');
  rejectUpdate(new Error('late rejection'));

  assert.equal(await updating, false);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
  assert.equal(root.events.some((event) => event.type === 'pliego:error'), false);
});

test('refresh waits for asynchronous cleanup before mounting again', async () => {
  const env = environment();
  const root = new FakeRoot();
  let releaseCleanup;
  let mounts = 0;
  const runtime = createAdapterRuntime(env, async () => ({
    mount() {
      mounts += 1;
      return () => new Promise((resolve) => { releaseCleanup = resolve; });
    },
  }));

  await runtime.mount(root);
  runtime.refresh(root);
  await nextTurn();
  assert.equal(mounts, 1);
  releaseCleanup();
  await nextTurn();
  assert.equal(mounts, 2);
  assert.equal(root.dataset.pliegoStatus, 'mounted');
});

test('installed runtime reacts to reduced-motion and Save-Data changes', async () => {
  const env = environment();
  const root = new FakeRoot({ pliegoMotion: 'skip', pliegoData: 'skip' });
  env.roots.push(root);
  let mounts = 0;
  let cleanups = 0;
  const runtime = createAdapterRuntime(env, async () => ({
    mount() {
      mounts += 1;
      return () => { cleanups += 1; };
    },
  }));

  runtime.install();
  await nextTurn();
  assert.equal(mounts, 1);
  env.media.set(true);
  await nextTurn();
  assert.equal(root.dataset.pliegoStatus, 'skipped');
  assert.equal(cleanups, 1);
  env.media.set(false);
  await nextTurn();
  assert.equal(mounts, 2);
  env.connection.setSaveData(true);
  await nextTurn();
  assert.equal(root.dataset.pliegoStatus, 'skipped');
  env.connection.setSaveData(false);
  await nextTurn();
  assert.equal(mounts, 3);
  runtime.destroy();
});

test('scope disposal aborts and cleans a mounted adapter synchronously', async () => {
  const env = environment();
  const root = new FakeRoot();
  env.roots.push(root);
  const calls = [];
  let signal;
  const runtime = createAdapterRuntime(env, async () => ({
    mount(_root, _props, context) {
      signal = context.signal;
      context.onCleanup(() => calls.push('registered'));
      return () => calls.push('returned');
    },
    unmount() {
      calls.push('unmount');
    },
  }));

  runtime.install();
  await nextTurn();
  env.document.emit('pliego:scope-dispose', {}, root);

  assert.equal(signal.aborted, true);
  assert.deepEqual(calls, ['unmount', 'returned', 'registered']);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
  runtime.destroy();
});

test('a mount that never settles cannot postpone registered cleanup', async () => {
  const env = environment();
  const root = new FakeRoot();
  const calls = [];
  let signal;
  let release;
  const runtime = createAdapterRuntime(env, async () => ({
    mount(_root, _props, context) {
      signal = context.signal;
      context.onCleanup(() => calls.push('registered'));
      return new Promise((resolve) => { release = resolve; });
    },
    unmount() {
      calls.push('unmount');
    },
  }));

  const mounting = runtime.mount(root);
  await nextTurn();
  assert.equal(runtime.unmount(root, 'scope-dispose'), true);
  assert.equal(signal.aborted, true);
  assert.deepEqual(calls, ['unmount', 'registered']);

  release(() => calls.push('returned'));
  assert.equal(await mounting, false);
  assert.deepEqual(calls, ['unmount', 'registered', 'returned']);
});

test('an update that never settles cannot postpone lifecycle cleanup', async () => {
  const env = environment();
  const root = new FakeRoot();
  const calls = [];
  let signal;
  const runtime = createAdapterRuntime(env, async () => ({
    mount(_root, _props, context) {
      signal = context.signal;
      context.onCleanup(() => calls.push('registered'));
    },
    update() {
      calls.push('update');
      return new Promise(() => {});
    },
    unmount() {
      calls.push('unmount');
    },
  }));

  await runtime.mount(root);
  void runtime.update(root, { pending: true });
  await nextTurn();
  assert.equal(runtime.unmount(root, 'scope-dispose'), true);

  assert.equal(signal.aborted, true);
  assert.deepEqual(calls, ['update', 'unmount', 'registered']);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
});

test('pagehide disposes known roots even after they leave the document query', async () => {
  const env = environment();
  const root = new FakeRoot();
  env.roots.push(root);
  let cleanups = 0;
  const runtime = createAdapterRuntime(env, async () => ({
    mount() {
      return () => { cleanups += 1; };
    },
  }));

  runtime.install();
  await nextTurn();
  env.roots.splice(0);
  root.isConnected = false;
  env.emit('pagehide');

  assert.equal(cleanups, 1);
  assert.equal(root.dataset.pliegoStatus, 'disposed');
  runtime.destroy();
});
