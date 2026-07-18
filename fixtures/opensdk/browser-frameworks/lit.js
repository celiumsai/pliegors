// SPDX-License-Identifier: Apache-2.0

import { LitElement, html } from "lit";

const TAG = "pliego-lit-status";
const metrics = frameworkMetrics("lit");

class PliegoLitStatus extends LitElement {
  static properties = {
    message: { type: String },
    motion: { type: String },
  };

  connectedCallback() {
    super.connectedCallback();
    if (!this.pliegoActive) {
      this.pliegoActive = true;
      metrics.mount();
    }
  }

  disconnectedCallback() {
    super.disconnectedCallback();
    this.dispose();
  }

  dispose() {
    if (!this.pliegoActive) return;
    this.pliegoActive = false;
    metrics.unmount();
  }

  render() {
    return html`<p data-framework="lit" data-motion=${this.motion || "unknown"}>${this.message || "Lit"}</p>`;
  }
}

if (!customElements.get(TAG)) customElements.define(TAG, PliegoLitStatus);
const instances = new WeakMap();

export const pliegoAdapter = Object.freeze({
  apiVersion: 1,
  async mount(root, props, context) {
    const element = document.createElement(TAG);
    element.message = props.message;
    element.motion = context.motion;
    instances.set(root, element);
    root.replaceChildren(element);
    await element.updateComplete;
  },
  async update(root, props, context) {
    const element = instances.get(root);
    if (!element) throw new Error("Lit component is not mounted");
    element.message = props.message;
    element.motion = context.motion;
    await element.updateComplete;
    metrics.update();
  },
  unmount(root) {
    const element = instances.get(root);
    instances.delete(root);
    element?.dispose();
    root.replaceChildren();
  },
});

function frameworkMetrics(name) {
  const state = globalThis.__pliegoFrameworkMetrics ||= { active: {}, events: [] };
  state.active[name] ||= 0;
  return {
    mount() { state.active[name] += 1; state.events.push(`${name}:mount`); },
    update() { state.events.push(`${name}:update`); },
    unmount() { state.active[name] -= 1; state.events.push(`${name}:unmount`); },
  };
}
