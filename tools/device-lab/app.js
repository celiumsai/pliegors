const roleSelect = document.querySelector("#device-role");
const runButton = document.querySelector("#run-probe");
const statusNode = document.querySelector("#status");
const titleNode = document.querySelector("#readout-title");
const summaryNode = document.querySelector("#summary");
const rawSection = document.querySelector(".raw-output");
const rawNode = document.querySelector("#raw-json");
const heroValue = document.querySelector("#hero-value");
const heroUnit = document.querySelector("#hero-unit");
const clockNode = document.querySelector("#clock");

selectLikelyRole();
showBrowserRequirement();
runButton.addEventListener("click", runProbe);

async function runProbe() {
  runButton.disabled = true;
  runButton.classList.add("is-running");
  statusNode.textContent = "Sampling rAF cadence for five seconds. Keep this tab visible.";
  titleNode.textContent = "Measuring physical device";
  heroValue.textContent = "LIVE";
  heroUnit.textContent = "do not leave this tab";

  try {
    const [deviceId, role] = roleSelect.value.split("|");
    const browserFamily = detectBrowserFamily();
    if (role === "ipad-safari" && browserFamily !== "safari-ios") {
      throw new Error("This acceptance row requires Safari. Open this URL in Safari and run the probe again.");
    }
    const report = {
      $schema: "https://pliegors.dev/schemas/pliego.device-fingerprint.schema.json",
      reportVersion: "1.0.0",
      capturedAt: new Date().toISOString(),
      deviceId,
      role,
      identity: await collectIdentity(),
      viewport: collectViewport(),
      preferences: collectPreferences(),
      network: collectNetwork(),
      webgl: collectWebgl(),
      frameProbe: await measureFrames(5000),
    };
    const response = await fetch("/api/fingerprints", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(report),
    });
    const receipt = await response.json();
    if (!response.ok) throw new Error(receipt.detail ?? receipt.error ?? "Capture rejected");

    heroValue.textContent = formatNumber(report.frameProbe.p95Ms);
    heroUnit.textContent = "ms / rAF interval";
    titleNode.textContent = "Physical fingerprint captured";
    statusNode.textContent = `Saved as ${receipt.fileName}`;
    clockNode.textContent = report.capturedAt;
    renderSummary(report, receipt);
    rawNode.textContent = JSON.stringify({ ...report, receipt }, null, 2);
    rawSection.hidden = false;
  } catch (error) {
    heroValue.textContent = "ERROR";
    heroUnit.textContent = "capture not saved";
    titleNode.textContent = "Probe stopped";
    statusNode.textContent = error instanceof Error ? error.message : String(error);
  } finally {
    runButton.disabled = false;
    runButton.classList.remove("is-running");
  }
}

async function collectIdentity() {
  const highEntropy = navigator.userAgentData?.getHighEntropyValues
    ? await navigator.userAgentData
        .getHighEntropyValues(["architecture", "bitness", "model", "platformVersion"])
        .catch(() => null)
    : null;
  return compact({
    userAgent: navigator.userAgent,
    browserFamily: detectBrowserFamily(),
    browserEngine: isIosFamily() ? "webkit" : "blink",
    platform: navigator.platform,
    vendor: navigator.vendor,
    language: navigator.language,
    languages: navigator.languages,
    hardwareConcurrency: navigator.hardwareConcurrency,
    deviceMemoryGiB: navigator.deviceMemory,
    maxTouchPoints: navigator.maxTouchPoints,
    userAgentData: highEntropy,
  });
}

function collectViewport() {
  return compact({
    innerWidth: window.innerWidth,
    innerHeight: window.innerHeight,
    visualWidth: window.visualViewport?.width,
    visualHeight: window.visualViewport?.height,
    screenWidth: window.screen.width,
    screenHeight: window.screen.height,
    availableWidth: window.screen.availWidth,
    availableHeight: window.screen.availHeight,
    devicePixelRatio: window.devicePixelRatio,
    orientation: window.screen.orientation?.type,
    colorDepth: window.screen.colorDepth,
  });
}

function collectPreferences() {
  return {
    reducedMotion: media("(prefers-reduced-motion: reduce)"),
    reducedTransparency: media("(prefers-reduced-transparency: reduce)"),
    highContrast: media("(prefers-contrast: more)"),
    darkScheme: media("(prefers-color-scheme: dark)"),
    wideColor: media("(color-gamut: p3)"),
    hdr: media("(dynamic-range: high)"),
    pointerCoarse: media("(pointer: coarse)"),
    hover: media("(hover: hover)"),
  };
}

function collectNetwork() {
  const connection =
    navigator.connection ?? navigator.mozConnection ?? navigator.webkitConnection;
  if (!connection) return null;
  return compact({
    effectiveType: connection.effectiveType,
    downlinkMbps: connection.downlink,
    rttMs: connection.rtt,
    saveData: connection.saveData,
  });
}

function collectWebgl() {
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("webgl2", { powerPreference: "default" }) ??
    canvas.getContext("webgl", { powerPreference: "default" });
  if (!context) return null;
  const debug = context.getExtension("WEBGL_debug_renderer_info");
  const renderer = debug
    ? context.getParameter(debug.UNMASKED_RENDERER_WEBGL)
    : context.getParameter(context.RENDERER);
  const vendor = debug
    ? context.getParameter(debug.UNMASKED_VENDOR_WEBGL)
    : context.getParameter(context.VENDOR);
  const isWebgl2 =
    typeof WebGL2RenderingContext !== "undefined" &&
    context instanceof WebGL2RenderingContext;
  return compact({
    version: isWebgl2 ? 2 : 1,
    renderer,
    vendor,
    shadingLanguageVersion: context.getParameter(context.SHADING_LANGUAGE_VERSION),
    maxTextureSize: context.getParameter(context.MAX_TEXTURE_SIZE),
    maxCubeMapTextureSize: context.getParameter(context.MAX_CUBE_MAP_TEXTURE_SIZE),
    maxRenderbufferSize: context.getParameter(context.MAX_RENDERBUFFER_SIZE),
    maxCombinedTextureUnits: context.getParameter(context.MAX_COMBINED_TEXTURE_IMAGE_UNITS),
    maxVertexUniformVectors: context.getParameter(context.MAX_VERTEX_UNIFORM_VECTORS),
    maxFragmentUniformVectors: context.getParameter(context.MAX_FRAGMENT_UNIFORM_VECTORS),
    antialias: context.getContextAttributes()?.antialias,
  });
}

function measureFrames(durationMs) {
  return new Promise((resolve) => {
    const deltas = [];
    let start;
    let previous;
    function sample(now) {
      start ??= now;
      if (previous !== undefined) deltas.push(now - previous);
      previous = now;
      const elapsed = now - start;
      const progress = Math.min(1, elapsed / durationMs);
      runButton.style.setProperty("--progress", `${progress * 100}%`);
      if (elapsed < durationMs) {
        requestAnimationFrame(sample);
        return;
      }
      deltas.sort((left, right) => left - right);
      const medianMs = percentile(deltas, 0.5);
      resolve({
        sampleCount: deltas.length,
        medianMs,
        p95Ms: percentile(deltas, 0.95),
        maxMs: round(deltas.at(-1) ?? 0),
        over33Ms: deltas.filter((delta) => delta > 33).length,
        estimatedRefreshHz: medianMs > 0 ? round(1000 / medianMs) : 0,
      });
    }
    requestAnimationFrame(sample);
  });
}

function renderSummary(report, receipt) {
  const rows = [
    ["Renderer", report.webgl?.renderer ?? "Unavailable"],
    ["Viewport", `${report.viewport.innerWidth} × ${report.viewport.innerHeight} @ ${report.viewport.devicePixelRatio}×`],
    [
      "rAF interval p95",
      `${formatNumber(report.frameProbe.p95Ms)} ms · ~${formatNumber(report.frameProbe.estimatedRefreshHz)} Hz`,
    ],
    ["Saved evidence", receipt.sha256.slice(0, 16)],
  ];
  summaryNode.replaceChildren(
    ...rows.map(([term, detail]) => {
      const wrapper = document.createElement("div");
      const dt = document.createElement("dt");
      const dd = document.createElement("dd");
      dt.textContent = term;
      dd.textContent = detail;
      wrapper.append(dt, dd);
      return wrapper;
    }),
  );
}

function selectLikelyRole() {
  const userAgent = navigator.userAgent;
  if (/Android/i.test(userAgent)) roleSelect.value = "android-modest-physical|modest-android";
  else if (/iPad/i.test(userAgent) || (/Macintosh/i.test(userAgent) && navigator.maxTouchPoints > 1)) {
    roleSelect.value = "ipad-safari-physical|ipad-safari";
  } else if (/Windows/i.test(userAgent)) {
    roleSelect.value = "windows-arc-140t-igpu|integrated-gpu-laptop";
  }
}

function showBrowserRequirement() {
  if (detectBrowserFamily() === "chrome-ios") {
    statusNode.textContent = "Chrome for iPad detected. Open this URL in Safari for the required acceptance capture.";
  }
}

function detectBrowserFamily() {
  const userAgent = navigator.userAgent;
  if (/CriOS\//i.test(userAgent)) return "chrome-ios";
  if (/FxiOS\//i.test(userAgent)) return "firefox-ios";
  if (/EdgiOS\//i.test(userAgent)) return "edge-ios";
  if (isIosFamily() && /Version\/[\d.]+.*Safari\//i.test(userAgent)) return "safari-ios";
  if (/Chrome\//i.test(userAgent)) return "chrome";
  if (/Safari\//i.test(userAgent)) return "safari";
  return "other";
}

function isIosFamily() {
  return (
    /iPad|iPhone|iPod/i.test(navigator.userAgent) ||
    (/Macintosh/i.test(navigator.userAgent) && navigator.maxTouchPoints > 1)
  );
}

function compact(value) {
  return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined));
}

function media(query) {
  return window.matchMedia(query).matches;
}

function percentile(sorted, quantile) {
  if (!sorted.length) return 0;
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * quantile) - 1);
  return round(sorted[index]);
}

function round(value) {
  return Math.round(value * 100) / 100;
}

function formatNumber(value) {
  return new Intl.NumberFormat("en", {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  }).format(value);
}
