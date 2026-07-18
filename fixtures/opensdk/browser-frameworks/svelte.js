// SPDX-License-Identifier: Apache-2.0

import { mount, unmount } from "svelte";
import Status from "./Status.svelte";

const TAG = "pliego-svelte-status";
const metrics = frameworkMetrics("svelte");

class PliegoSvelteStatus extends HTMLElement {
  connectedCallback() {
    void this.renderSvelte();
  }

  async setState(value) {
    this.currentState = value;
    if (this.isConnected) await this.renderSvelte();
  }

  async renderSvelte() {
    const generation = (this.generation || 0) + 1;
    this.generation = generation;
    if (this.component) {
      const previous = this.component;
      this.component = null;
      await unmount(previous);
      metrics.unmount();
    }
    if (!this.isConnected || this.generation !== generation) return;
    const state = this.currentState || {};
    this.component = mount(Status, {
      target: this,
      props: { message: state.message || "Svelte", motion: state.motion || "unknown" },
    });
    metrics.mount();
  }

  async dispose() {
    this.generation = (this.generation || 0) + 1;
    if (!this.component) return;
    const component = this.component;
    this.component = null;
    await unmount(component);
    metrics.unmount();
  }

  disconnectedCallback() {
    void this.dispose();
  }
}

if (!customElements.get(TAG)) customElements.define(TAG, PliegoSvelteStatus);
const instances = new WeakMap();

export const pliegoAdapter = Object.freeze({
  apiVersion: 1,
  async mount(root, props, context) {
    const element = document.createElement(TAG);
    instances.set(root, element);
    root.replaceChildren(element);
    await element.setState({ ...props, motion: context.motion });
  },
  async update(root, props, context) {
    const element = instances.get(root);
    if (!element) throw new Error("Svelte component is not mounted");
    await element.setState({ ...props, motion: context.motion });
    metrics.update();
  },
  async unmount(root) {
    const element = instances.get(root);
    instances.delete(root);
    await element?.dispose();
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
