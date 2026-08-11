// SPDX-License-Identifier: AGPL-3.0-only

export const pliegoComponent = Object.freeze({
  apiVersion: "0.2.0-beta.1",
  tagName: "pliego-status",
  capabilities: Object.freeze(["dom"]),
  register(registry = globalThis.customElements) {
    if (!registry || registry.get(this.tagName)) return;
    registry.define(
      this.tagName,
      class PliegoStatus extends HTMLElement {
        connectedCallback() {
          this.textContent = this.getAttribute("message") || "PliegoRS";
        }
      },
    );
  },
});
