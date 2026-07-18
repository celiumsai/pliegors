// SPDX-License-Identifier: Apache-2.0

import React from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";

const TAG = "pliego-react-status";
const metrics = frameworkMetrics("react");

class PliegoReactStatus extends HTMLElement {
  connectedCallback() {
    if (this.reactRoot) return;
    this.reactRoot = createRoot(this);
    metrics.mount();
    this.renderReact();
  }

  set state(value) {
    this.currentState = value;
    this.renderReact();
  }

  renderReact() {
    if (!this.reactRoot) return;
    const state = this.currentState || {};
    flushSync(() => {
      this.reactRoot.render(React.createElement(
        "p",
        { "data-framework": "react", "data-motion": state.motion || "unknown" },
        state.message || "React",
      ));
    });
  }

  dispose() {
    if (!this.reactRoot) return;
    this.reactRoot.unmount();
    this.reactRoot = null;
    metrics.unmount();
  }

  disconnectedCallback() {
    this.dispose();
  }
}

if (!customElements.get(TAG)) customElements.define(TAG, PliegoReactStatus);
const instances = new WeakMap();

export const pliegoAdapter = Object.freeze({
  apiVersion: 1,
  mount(root, props, context) {
    const element = document.createElement(TAG);
    element.state = { ...props, motion: context.motion };
    root.replaceChildren(element);
    instances.set(root, element);
  },
  update(root, props, context) {
    const element = instances.get(root);
    if (!element) throw new Error("React component is not mounted");
    element.state = { ...props, motion: context.motion };
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
