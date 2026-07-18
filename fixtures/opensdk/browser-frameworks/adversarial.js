// SPDX-License-Identifier: Apache-2.0

const TAG = "pliego-adversarial-status";
const state = globalThis.__pliegoFrameworkMetrics ||= { active: {}, events: [] };
state.active.adversarial ||= 0;
state.resources ||= { timers: 0, listeners: 0, scopes: 0, contexts: 0, ticks: 0 };

class PliegoAdversarialStatus extends HTMLElement {}
if (!customElements.get(TAG)) customElements.define(TAG, PliegoAdversarialStatus);

const instances = new WeakMap();

export const pliegoAdapter = Object.freeze({
  apiVersion: 1,
  mount(root, props, context) {
    const element = document.createElement(TAG);
    element.innerHTML = `<p data-framework="adversarial" data-motion="${context.motion}"></p>`;
    element.querySelector("p").textContent = props.message;
    root.replaceChildren(element);

    const scope = new AbortController();
    const channel = new MessageChannel();
    const listener = () => { state.resources.ticks += 1; };
    const timer = setInterval(() => { state.resources.ticks += 1; }, 5);
    document.addEventListener("pliego:adversarial-probe", listener);
    state.active.adversarial += 1;
    state.resources.timers += 1;
    state.resources.listeners += 1;
    state.resources.scopes += 1;
    state.resources.contexts += 1;
    state.events.push("adversarial:mount");

    let disposed = false;
    const cleanup = () => {
      if (disposed) return;
      disposed = true;
      clearInterval(timer);
      document.removeEventListener("pliego:adversarial-probe", listener);
      scope.abort();
      channel.port1.close();
      channel.port2.close();
      state.active.adversarial -= 1;
      state.resources.timers -= 1;
      state.resources.listeners -= 1;
      state.resources.scopes -= 1;
      state.resources.contexts -= 1;
      state.events.push("adversarial:unmount");
      root.replaceChildren();
      instances.delete(root);
    };
    instances.set(root, { element, cleanup });
    context.onCleanup(cleanup);
    return cleanup;
  },
  update(root, props, context) {
    const instance = instances.get(root);
    if (!instance) throw new Error("adversarial component is not mounted");
    const paragraph = instance.element.querySelector("p");
    paragraph.textContent = props.message;
    paragraph.dataset.motion = context.motion;
    state.events.push("adversarial:update");
  },
  unmount(root) {
    instances.get(root)?.cleanup();
  },
});
