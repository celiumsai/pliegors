// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

export const API_VERSION = 1;
export const RUNTIME_VERSION = '1.0.0';

const ROOT_SELECTOR = 'pliego-adapter';
const TIERS = Object.freeze(['universal', 'lite', 'balanced', 'signature']);
const TRIGGERS = Object.freeze(['immediate', 'visible', 'idle', 'interaction']);
const MOTION_POLICIES = Object.freeze(['auto', 'full', 'reduce', 'skip']);
const DATA_POLICIES = Object.freeze(['auto', 'allow', 'skip']);
const CAPABILITY_TIER = Object.freeze({
  dom: 'universal',
  motion: 'universal',
  'smooth-scroll': 'lite',
  audio: 'lite',
  video: 'lite',
  webgl: 'balanced',
  'high-frequency-raf': 'balanced',
  webgpu: 'signature',
});

const defaultImporter = (specifier) => import(specifier);

function utf8Length(value) {
  let bytes = 0;
  for (const character of value) {
    const codePoint = character.codePointAt(0);
    bytes += codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
  }
  return bytes;
}

function report(env, root, phase, error) {
  const cause = error instanceof Error ? error : new Error(String(error));
  root.dataset.pliegoStatus = 'error';
  try {
    root.dispatchEvent(new env.CustomEvent('pliego:error', {
      bubbles: true,
      detail: Object.freeze({ phase, message: cause.message }),
    }));
  } catch (_) {
    // A hostile event override must not escape the adapter boundary.
  }
  env.console?.error?.(`PLIEGO adapter ${phase} failed`, cause);
}

function emit(env, root, name, detail = {}) {
  try {
    root.dispatchEvent(new env.CustomEvent(name, {
      bubbles: true,
      detail: Object.freeze(detail),
    }));
  } catch (error) {
    env.console?.error?.(`PLIEGO adapter event ${name} failed`, error);
  }
}

function safeModulePath(env, value) {
  if (typeof value !== 'string'
    || !value.startsWith('/assets/')
    || !/^\/assets\/[A-Za-z0-9/_\-.]+\.js$/u.test(value)
    || value.includes('//')
    || value.includes('..')
    || value.includes('\\')
    || value.includes('%')
    || value.includes('?')
    || value.includes('#')
    || /[\u0000-\u001f\u007f]/u.test(value)) return null;
  try {
    const url = new URL(value, env.location.href);
    if (!['http:', 'https:'].includes(url.protocol)
      || url.origin !== env.location.origin
      || url.pathname !== value) return null;
    return url.pathname;
  } catch (_) {
    return null;
  }
}

function parseProps(root) {
  const source = root.dataset.pliegoProps || '{}';
  if (utf8Length(source) > 32768) throw new Error('adapter props exceed 32768 bytes');
  const value = JSON.parse(source);
  if (value === null || Array.isArray(value) || typeof value !== 'object') {
    throw new Error('adapter props must be a JSON object');
  }
  return Object.freeze(value);
}

function normalizeProps(value) {
  if (value === null || Array.isArray(value) || typeof value !== 'object') {
    throw new Error('adapter update props must be an object');
  }
  const source = JSON.stringify(value);
  if (utf8Length(source) > 32768) throw new Error('adapter props exceed 32768 bytes');
  return Object.freeze(JSON.parse(source));
}

function parseCapabilities(root) {
  const source = root.dataset.pliegoCapabilities || '';
  const capabilities = source ? source.split(',') : [];
  for (const capability of capabilities) {
    if (!Object.prototype.hasOwnProperty.call(CAPABILITY_TIER, capability)) {
      throw new Error(`unknown adapter capability: ${capability}`);
    }
  }
  return Object.freeze(capabilities);
}

function tierIndex(value) {
  return TIERS.indexOf(value);
}

function policyFor(env, root) {
  const capabilities = parseCapabilities(root);
  const tier = root.dataset.pliegoTier
    || env.document.documentElement.dataset.pliegoTier
    || 'balanced';
  const minimum = root.dataset.pliegoMinTier || 'universal';
  if (tierIndex(tier) < 0 || tierIndex(minimum) < 0) {
    throw new Error('adapter tier is invalid');
  }
  let required = tierIndex(minimum);
  for (const capability of capabilities) {
    required = Math.max(required, tierIndex(CAPABILITY_TIER[capability]));
  }
  const saveData = Boolean(env.navigator?.connection?.saveData);
  const dataPolicy = root.dataset.pliegoData || 'auto';
  const motionPolicy = root.dataset.pliegoMotion || 'auto';
  if (!DATA_POLICIES.includes(dataPolicy) || !MOTION_POLICIES.includes(motionPolicy)) {
    throw new Error('adapter motion or data policy is invalid');
  }
  const authoredMotion = env.document.documentElement.dataset.pliegoMotion;
  const prefersReduced = authoredMotion === 'reduced'
    || Boolean(env.matchMedia?.('(prefers-reduced-motion: reduce)').matches);
  const motion = motionPolicy === 'full'
    ? 'full'
    : motionPolicy === 'reduce' || prefersReduced
      ? 'reduced'
      : 'full';
  if (tierIndex(tier) < required) {
    return { allowed: false, reason: 'tier', tier, motion, saveData, capabilities };
  }
  if (saveData && dataPolicy === 'skip') {
    return { allowed: false, reason: 'save-data', tier, motion, saveData, capabilities };
  }
  if (motionPolicy === 'skip' && prefersReduced) {
    return { allowed: false, reason: 'reduced-motion', tier, motion: 'reduced', saveData, capabilities };
  }
  return { allowed: true, reason: null, tier, motion, saveData, capabilities };
}

function pluginFrom(module) {
  const plugin = module.pliegoAdapter
    || (module.default && typeof module.default.mount === 'function' ? module.default : module);
  if (!plugin || typeof plugin.mount !== 'function') {
    throw new Error('adapter module must export mount(root, props, context)');
  }
  const version = plugin.apiVersion ?? module.apiVersion;
  if (version !== undefined && version !== API_VERSION) {
    throw new Error(`adapter API ${version} is incompatible with runtime API ${API_VERSION}`);
  }
  return plugin;
}

function runCleanup(env, root, callback, phase) {
  if (typeof callback !== 'function') return;
  try {
    const result = callback();
    if (result && typeof result.then === 'function') {
      return Promise.resolve(result).catch((error) => {
        emit(env, root, 'pliego:cleanup-error', { phase, message: String(error?.message || error) });
        env.console?.error?.(`PLIEGO adapter ${phase} cleanup failed`, error);
      });
    }
  } catch (error) {
    emit(env, root, 'pliego:cleanup-error', { phase, message: String(error?.message || error) });
    env.console?.error?.(`PLIEGO adapter ${phase} cleanup failed`, error);
  }
}

export function createAdapterRuntime(env = globalThis, importer = defaultImporter) {
  const lifecycles = new WeakMap();
  const pending = new WeakMap();
  const mountTasks = new WeakMap();
  const updateQueues = new WeakMap();
  const updateEpochs = new WeakMap();
  const generations = new WeakMap();
  const teardowns = new WeakMap();
  const scheduled = new WeakMap();
  const knownRoots = new Set();
  let observer = null;
  let installed = false;
  const runtimeCleanups = [];

  const visibility = typeof env.IntersectionObserver === 'function'
    ? new env.IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting || !scheduled.has(entry.target)) continue;
        visibility.unobserve(entry.target);
        scheduled.delete(entry.target);
        void mount(entry.target);
      }
    })
    : null;

  function cancelSchedule(root) {
    const cancel = scheduled.get(root);
    if (cancel) void runCleanup(env, root, cancel, 'schedule');
    scheduled.delete(root);
    visibility?.unobserve(root);
  }

  function generation(root) {
    return generations.get(root) || 0;
  }

  function advanceGeneration(root) {
    const next = generation(root) + 1;
    generations.set(root, next);
    return next;
  }

  function updateEpoch(root) {
    return updateEpochs.get(root) || 0;
  }

  function invalidateUpdates(root) {
    updateEpochs.set(root, updateEpoch(root) + 1);
  }

  function isCurrent(root, record) {
    return pending.get(root) === record
      && generation(root) === record.generation
      && !record.controller.signal.aborted;
  }

  function clearPending(root, record) {
    if (pending.get(root) === record) pending.delete(root);
  }

  function trackTeardown(root, work) {
    const previous = teardowns.get(root);
    const barrier = (async () => {
      if (previous) await previous.catch(() => {});
      await Promise.resolve(work).catch((error) => {
        env.console?.error?.('PLIEGO adapter teardown barrier failed', error);
      });
    })();
    teardowns.set(root, barrier);
    barrier.then(() => {
      if (teardowns.get(root) === barrier) teardowns.delete(root);
    });
    return barrier;
  }

  function cleanupLifecycle(root, lifecycle, prefix = '') {
    const phase = (name) => prefix ? `${prefix}-${name}` : name;
    let barrier = null;
    const enqueue = (callback, cleanupPhase) => {
      if (typeof callback !== 'function') return;
      if (barrier) {
        barrier = barrier.then(() => runCleanup(env, root, callback, cleanupPhase));
        return;
      }
      const result = runCleanup(env, root, callback, cleanupPhase);
      if (result && typeof result.then === 'function') barrier = result;
    };
    enqueue(
      () => lifecycle.plugin?.unmount?.(root, lifecycle.context),
      phase('unmount-hook'),
    );
    enqueue(lifecycle.returnedCleanup, phase('mount-return'));
    for (let index = lifecycle.cleanups.length - 1; index >= 0; index -= 1) {
      enqueue(lifecycle.cleanups[index], phase('registered'));
    }
    return barrier || Promise.resolve();
  }

  function contextFor(root, controller, policy, cleanups) {
    return Object.freeze({
      apiVersion: API_VERSION,
      runtimeVersion: RUNTIME_VERSION,
      signal: controller.signal,
      tier: policy.tier,
      motion: policy.motion,
      saveData: policy.saveData,
      capabilities: policy.capabilities,
      onCleanup(callback) {
        if (typeof callback !== 'function') throw new TypeError('cleanup must be a function');
        if (controller.signal.aborted) void runCleanup(env, root, callback, 'late-registration');
        else cleanups.push(callback);
        return callback;
      },
    });
  }

  async function mountInternal(root, record, priorTeardown) {
    if (priorTeardown) await priorTeardown.catch(() => {});
    if (!isCurrent(root, record) || !root.isConnected) return false;

    const { controller } = record;
    const cleanups = [];
    let plugin = null;
    let context = null;
    let returnedCleanup;
    try {
      if ((root.dataset.pliegoApi || String(API_VERSION)) !== String(API_VERSION)) {
        throw new Error(`island API ${root.dataset.pliegoApi} is incompatible with runtime API ${API_VERSION}`);
      }
      const modulePath = safeModulePath(env, root.dataset.pliegoModule);
      if (!modulePath) throw new Error('adapter module path is not a safe same-origin /assets/ URL');
      const policy = policyFor(env, root);
      if (!policy.allowed) {
        controller.abort(policy.reason);
        if (generation(root) === record.generation) {
          root.dataset.pliegoStatus = 'skipped';
          root.dataset.pliegoSkipReason = policy.reason;
          emit(env, root, 'pliego:skip', { reason: policy.reason });
        }
        return false;
      }
      delete root.dataset.pliegoSkipReason;
      const props = parseProps(root);
      const module = await importer(modulePath);
      if (!isCurrent(root, record) || !root.isConnected) return false;
      plugin = pluginFrom(module);
      context = contextFor(root, controller, policy, cleanups);
      returnedCleanup = await plugin.mount(root, props, context);
      if (!isCurrent(root, record) || !root.isConnected) {
        await cleanupLifecycle(root, {
          controller,
          plugin,
          context,
          cleanups,
          returnedCleanup,
          props,
        }, 'abandoned');
        return false;
      }
      lifecycles.set(root, {
        controller,
        plugin,
        context,
        cleanups,
        returnedCleanup,
        props,
        policy,
      });
      root.dataset.pliegoStatus = 'mounted';
      emit(env, root, 'pliego:mount', { apiVersion: API_VERSION });
      return true;
    } catch (error) {
      if (controller.signal.aborted || generation(root) !== record.generation) {
        if (plugin && context) await cleanupLifecycle(root, {
          controller,
          plugin,
          context,
          cleanups,
          returnedCleanup,
          props: null,
        }, 'aborted');
        return false;
      }
      controller.abort(error);
      if (plugin && context) await cleanupLifecycle(root, {
        controller,
        plugin,
        context,
        cleanups,
        returnedCleanup,
        props: null,
      }, 'failed');
      report(env, root, 'mount', error);
      return false;
    }
  }

  function mount(root) {
    if (!root || lifecycles.has(root) || mountTasks.has(root)) return Promise.resolve(false);
    cancelSchedule(root);
    knownRoots.add(root);
    root.dataset.pliegoStatus = 'loading';
    const Controller = env.AbortController || globalThis.AbortController;
    const record = {
      controller: new Controller(),
      generation: advanceGeneration(root),
    };
    pending.set(root, record);
    const priorTeardown = teardowns.get(root);
    let resolveTask;
    let rejectTask;
    const task = new Promise((resolve, reject) => {
      resolveTask = resolve;
      rejectTask = reject;
    });
    mountTasks.set(root, task);
    mountInternal(root, record, priorTeardown).then(resolveTask, rejectTask);
    task.then(() => {
      clearPending(root, record);
      if (mountTasks.get(root) === task) mountTasks.delete(root);
    }, () => {
      clearPending(root, record);
      if (mountTasks.get(root) === task) mountTasks.delete(root);
    });
    return task;
  }

  async function performUpdate(root, preparedProps, epoch) {
    const activeMount = mountTasks.get(root);
    if (activeMount) await activeMount.catch(() => false);
    if (updateEpoch(root) !== epoch) return false;

    let lifecycle = lifecycles.get(root);
    if (!lifecycle) {
      await mount(root);
      if (updateEpoch(root) !== epoch) return false;
      lifecycle = lifecycles.get(root);
      if (!lifecycle) return false;
    }
    try {
      const props = preparedProps === undefined ? parseProps(root) : preparedProps;
      if (typeof lifecycle.plugin.update !== 'function') {
        dispose(root, 'update-remount', false);
        if (preparedProps !== undefined) root.dataset.pliegoProps = JSON.stringify(preparedProps);
        delete root.dataset.pliegoStatus;
        const mounted = await mount(root);
        return updateEpoch(root) === epoch && mounted;
      }
      await lifecycle.plugin.update(root, props, lifecycle.context);
      if (lifecycles.get(root) !== lifecycle
        || lifecycle.controller.signal.aborted
        || updateEpoch(root) !== epoch) return false;
      lifecycle.props = props;
      if (preparedProps !== undefined) root.dataset.pliegoProps = JSON.stringify(preparedProps);
      root.dataset.pliegoStatus = 'mounted';
      emit(env, root, 'pliego:update', { apiVersion: API_VERSION });
      return true;
    } catch (error) {
      if (lifecycles.get(root) !== lifecycle
        || lifecycle.controller.signal.aborted
        || updateEpoch(root) !== epoch) return false;
      report(env, root, 'update', error);
      return false;
    }
  }

  function update(root, nextProps) {
    let preparedProps;
    try {
      preparedProps = nextProps === undefined ? undefined : normalizeProps(nextProps);
    } catch (error) {
      report(env, root, 'update', error);
      return Promise.resolve(false);
    }
    const epoch = updateEpoch(root);
    const previous = updateQueues.get(root);
    const task = previous
      ? previous.catch(() => false).then(() => performUpdate(root, preparedProps, epoch))
      : performUpdate(root, preparedProps, epoch);
    updateQueues.set(root, task);
    task.then(() => {
      if (updateQueues.get(root) === task) updateQueues.delete(root);
    }, () => {
      if (updateQueues.get(root) === task) updateQueues.delete(root);
    });
    return task;
  }

  function dispose(root, reason = 'manual', invalidateUpdateQueue = true) {
    cancelSchedule(root);
    knownRoots.delete(root);
    advanceGeneration(root);
    if (invalidateUpdateQueue) invalidateUpdates(root);

    const record = pending.get(root);
    if (record) {
      record.controller.abort(reason);
      clearPending(root, record);
    }
    const activeMount = mountTasks.get(root);
    if (activeMount && mountTasks.get(root) === activeMount) mountTasks.delete(root);

    const lifecycle = lifecycles.get(root);
    if (lifecycle) {
      lifecycles.delete(root);
      lifecycle.controller.abort(reason);
    }

    const activeUpdate = invalidateUpdateQueue ? updateQueues.get(root) : null;
    if (lifecycle) {
      const work = (async () => {
        if (activeUpdate) await activeUpdate.catch(() => false);
        await cleanupLifecycle(root, lifecycle);
      })();
      trackTeardown(root, work);
    } else if (activeMount || activeUpdate) {
      const work = (async () => {
        if (activeMount) await activeMount.catch(() => false);
        if (activeUpdate) await activeUpdate.catch(() => false);
      })();
      trackTeardown(root, work);
    }

    if (root?.dataset) root.dataset.pliegoStatus = 'disposed';
    if (lifecycle) emit(env, root, 'pliego:unmount', { reason });
    return Boolean(lifecycle);
  }

  function unmount(root, reason = 'manual') {
    return dispose(root, reason, true);
  }

  function schedule(root) {
    if (!root || lifecycles.has(root) || scheduled.has(root) || root.dataset.pliegoStatus === 'loading') return;
    knownRoots.add(root);
    const trigger = root.dataset.pliegoTrigger || 'visible';
    if (!TRIGGERS.includes(trigger)) {
      report(env, root, 'schedule', new Error(`unknown adapter trigger: ${trigger}`));
      return;
    }
    root.dataset.pliegoStatus = 'scheduled';
    if (trigger === 'visible' && visibility) {
      visibility.observe(root);
      scheduled.set(root, () => visibility.unobserve(root));
      return;
    }
    if (trigger === 'idle') {
      const callback = () => {
        if (!scheduled.has(root)) return;
        scheduled.delete(root);
        void mount(root);
      };
      if (typeof env.requestIdleCallback === 'function') {
        const handle = env.requestIdleCallback(callback, { timeout: 2000 });
        scheduled.set(root, () => env.cancelIdleCallback?.(handle));
      } else {
        const handle = env.setTimeout(callback, 1);
        scheduled.set(root, () => env.clearTimeout(handle));
      }
      return;
    }
    if (trigger === 'interaction') {
      const activate = () => {
        cancelSchedule(root);
        void mount(root);
      };
      const options = { capture: true, once: true };
      for (const name of ['pointerdown', 'keydown', 'focusin', 'touchstart']) {
        root.addEventListener(name, activate, options);
      }
      scheduled.set(root, () => {
        for (const name of ['pointerdown', 'keydown', 'focusin', 'touchstart']) {
          root.removeEventListener(name, activate, options);
        }
      });
      return;
    }
    void mount(root);
  }

  function rootsWithin(parent) {
    const roots = [];
    if (parent?.matches?.(ROOT_SELECTOR)) roots.push(parent);
    for (const root of parent?.querySelectorAll?.(ROOT_SELECTOR) || []) roots.push(root);
    return roots;
  }

  function scan(parent = env.document) {
    for (const root of rootsWithin(parent)) schedule(root);
  }

  function refresh(root) {
    unmount(root, 'refresh');
    delete root.dataset.pliegoStatus;
    schedule(root);
  }

  function reconcilePolicies() {
    const roots = new Set([...knownRoots, ...rootsWithin(env.document)]);
    for (const root of roots) refresh(root);
  }

  function listen(target, name, callback) {
    target?.addEventListener?.(name, callback);
    runtimeCleanups.push(() => target?.removeEventListener?.(name, callback));
  }

  function install() {
    if (installed) {
      scan();
      return;
    }
    installed = true;
    scan();
    if (typeof env.MutationObserver === 'function') {
      observer = new env.MutationObserver((records) => {
        for (const record of records) {
          for (const node of record.addedNodes) {
            if (node?.nodeType === 1) scan(node);
          }
          for (const node of record.removedNodes) {
            if (node?.nodeType !== 1) continue;
            for (const root of rootsWithin(node)) {
              (env.queueMicrotask || ((callback) => Promise.resolve().then(callback)))(() => {
                if (!root.isConnected) unmount(root, 'dom-removal');
              });
            }
          }
        }
      });
      observer.observe(env.document.documentElement, { childList: true, subtree: true });
    }
    const onPageHide = () => {
      for (const root of rootsWithin(env.document)) unmount(root, 'pagehide');
    };
    const onPageShow = (event) => {
      if (event.persisted) scan();
    };
    const onAdapterUpdate = (event) => {
      const root = event.target?.closest?.(ROOT_SELECTOR);
      if (root) void update(root, event.detail?.props);
    };
    listen(env, 'pagehide', onPageHide);
    listen(env, 'pageshow', onPageShow);
    listen(env.document, 'pliego:adapter-update', onAdapterUpdate);

    const motionQuery = env.matchMedia?.('(prefers-reduced-motion: reduce)');
    if (motionQuery?.addEventListener) listen(motionQuery, 'change', reconcilePolicies);
    else if (motionQuery?.addListener) {
      motionQuery.addListener(reconcilePolicies);
      runtimeCleanups.push(() => motionQuery.removeListener?.(reconcilePolicies));
    }
    listen(env.navigator?.connection, 'change', reconcilePolicies);
  }

  function destroy() {
    observer?.disconnect();
    observer = null;
    visibility?.disconnect();
    const roots = new Set([...knownRoots, ...rootsWithin(env.document)]);
    for (const root of roots) unmount(root, 'runtime-destroy');
    for (const cleanup of runtimeCleanups.splice(0)) cleanup();
    installed = false;
  }

  return Object.freeze({
    apiVersion: API_VERSION,
    runtimeVersion: RUNTIME_VERSION,
    mount,
    update,
    unmount,
    schedule,
    scan,
    refresh,
    destroy,
    install,
  });
}

if (typeof document !== 'undefined' && document.documentElement) {
  const existing = globalThis.PliegoAdapters;
  if (!existing) {
    const runtime = createAdapterRuntime(globalThis);
    Object.defineProperty(globalThis, 'PliegoAdapters', {
      value: runtime,
      configurable: false,
      enumerable: true,
      writable: false,
    });
    runtime.install();
  } else if (existing.apiVersion === API_VERSION) {
    if (typeof existing.install === 'function') existing.install();
    else if (typeof existing.scan === 'function') existing.scan();
    else console.error('PLIEGO adapter runtime v1 exists without install or scan');
  } else {
    console.error(`PLIEGO adapter runtime conflict: ${existing.apiVersion} != ${API_VERSION}`);
  }
}
