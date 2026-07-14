const form = document.querySelector("#run-form");
const deviceSelect = document.querySelector("#device-id");
const fingerprintSelect = document.querySelector("#hardware-fingerprint");
const targetSelect = document.querySelector("#target-id");
const routeSelect = document.querySelector("#route");
const tierSelect = document.querySelector("#tier");
const orientationSelect = document.querySelector("#orientation");
const networkInput = document.querySelector("#network-profile");
const startButton = document.querySelector("#start-run");
const statusNode = document.querySelector("#setup-status");
const sourceNode = document.querySelector("#source-revision");
const countNode = document.querySelector("#run-count");
const boundTitle = document.querySelector("#bound-title");
const rendererNode = document.querySelector("#renderer-value");
const endpointNode = document.querySelector("#endpoint-value");
const captureNode = document.querySelector("#capture-value");
const deviceLabLink = document.querySelector("#device-lab-link");

let config;

deviceLabLink.href = `${location.protocol}//${location.hostname}:5310/`;
form.addEventListener("submit", startRun);
deviceSelect.addEventListener("change", updateDevice);
targetSelect.addEventListener("change", updateTarget);
fingerprintSelect.addEventListener("change", updateBoundRow);
routeSelect.addEventListener("change", updateBoundRow);
tierSelect.addEventListener("change", updateBoundRow);

loadConfig();

async function loadConfig() {
  try {
    const response = await fetch("/_pliego/api/config", { cache: "no-store" });
    config = await response.json();
    if (!response.ok) throw new Error(config.detail ?? config.error ?? "Configuration unavailable");

    sourceNode.textContent = `${config.contractRevision} / route artifact sealed at start`;
    countNode.textContent = String(config.inboxRunCount).padStart(2, "0");
    replaceOptions(
      deviceSelect,
      config.devices,
      (device) => device.id,
      (device) => `${device.id} / ${device.status}`,
    );
    replaceOptions(targetSelect, config.targets, (target) => target.id, (target) => target.id);
    selectLikelyDevice();
    updateDevice();
    updateTarget();
    startButton.disabled = false;
    statusNode.textContent = "The next page begins recording at document load.";
  } catch (error) {
    statusNode.textContent = error instanceof Error ? error.message : String(error);
  }
}

function updateDevice() {
  const device = selectedDevice();
  if (!device) return;
  const fingerprints = config.fingerprints.filter(
    (fingerprint) => fingerprint.deviceId === device.id,
  );
  replaceOptions(
    fingerprintSelect,
    fingerprints,
    (fingerprint) => fingerprint.fileName,
    (fingerprint) => `${fingerprint.capturedAt.slice(0, 10)} / ${fingerprint.browser}`,
  );
  replaceOptions(tierSelect, device.tiers, (tier) => tier, (tier) => tier);
  networkInput.value = device.networkProfile;
  orientationSelect.value = device.role === "modest-android" ? "portrait" : "landscape";
  const power = document.querySelector("#power-state");
  power.value = device.role.includes("laptop") ? "external-power" : "battery-over-50";
  startButton.disabled = fingerprints.length === 0;
  statusNode.textContent = fingerprints.length
    ? "The next page begins recording at document load."
    : "This device row has no accepted fingerprint yet.";
  updateBoundRow();
}

function updateTarget() {
  const target = selectedTarget();
  if (!target) return;
  replaceOptions(routeSelect, target.routes, (route) => route, (route) => route);
  updateBoundRow();
}

function updateBoundRow() {
  if (!config) return;
  const target = selectedTarget();
  const fingerprint = config.fingerprints.find(
    (candidate) => candidate.fileName === fingerprintSelect.value,
  );
  boundTitle.textContent = `${target?.id ?? "No target"} / ${routeSelect.value || "no route"}`;
  rendererNode.textContent = fingerprint?.renderer ?? "No accepted fingerprint";
  endpointNode.textContent = target?.endpoint ?? "Pending";
  captureNode.textContent = `${tierSelect.value || "no tier"} / ${orientationSelect.value}`;
}

async function startRun(event) {
  event.preventDefault();
  startButton.disabled = true;
  statusNode.textContent = "Binding the run and clearing the requested cache state.";
  try {
    const data = new FormData(form);
    const payload = Object.fromEntries(data.entries());
    const response = await fetch("/_pliego/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    const result = await response.json();
    if (!response.ok) {
      throw new Error(result.details?.join("; ") ?? result.detail ?? result.error);
    }
    statusNode.textContent = `Run ${result.session.id.slice(0, 8)} bound. Opening ${result.launchUrl}.`;
    location.assign(result.launchUrl);
  } catch (error) {
    statusNode.textContent = error instanceof Error ? error.message : String(error);
    startButton.disabled = false;
  }
}

function replaceOptions(select, values, getValue, getLabel) {
  select.replaceChildren(
    ...values.map((value) => {
      const option = document.createElement("option");
      option.value = getValue(value);
      option.textContent = getLabel(value);
      return option;
    }),
  );
}

function selectedDevice() {
  return config?.devices.find((device) => device.id === deviceSelect.value);
}

function selectedTarget() {
  return config?.targets.find((target) => target.id === targetSelect.value);
}

function selectLikelyDevice() {
  const userAgent = navigator.userAgent;
  if (/Android/i.test(userAgent)) deviceSelect.value = "android-modest-physical";
  else if (/iPad/i.test(userAgent) || (/Macintosh/i.test(userAgent) && navigator.maxTouchPoints > 1)) {
    deviceSelect.value = "ipad-safari-physical";
  } else if (/Windows/i.test(userAgent)) {
    deviceSelect.value = "windows-arc-140t-igpu";
  }
}
