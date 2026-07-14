(() => {
  const probeLogic = window.__PLIEGO_ROUTE_PROBE_LOGIC__;
  const backgroundConfirmationMs = 500;
  const minimumRunDurationMs = 10_000;
  const stepLabels = Object.freeze({
    ready: "Ready mark",
    scroll: "Scroll one viewport",
    navigation: "Primary navigation",
    visualResponse: "Visual response",
    disclosure: "Menu or disclosure",
    sceneHold: "Scene hold",
    returnToInitial: "Return to route",
  });
  let segmentStartedAt = performance.now();
  const frameDeltas = [];
  const longTaskDurations = [];
  const interactionDurations = [];
  const decodeDurations = [];
  const violations = new Set();
  const supported = new Set(PerformanceObserver.supportedEntryTypes ?? []);
  const initialViewport = {
    width: window.innerWidth,
    height: window.innerHeight,
    devicePixelRatio: window.devicePixelRatio,
  };
  let previousFrame;
  let resourceBaseline = 0;
  let includeNavigation = true;
  let lcpMs = null;
  const layoutShifts = [];
  let leftForeground = false;
  let pageHiding = false;
  let backgroundTimer;
  let session;
  let persisted;
  let storageKey;
  let submitted = false;
  let panel;
  let statusNode;
  let timerNode;
  let finishButton;
  let timerHandle;
  let recording = true;
  let initialSnapshot;

  if (!probeLogic) {
    throw new Error("PLIEGO probe contract unavailable");
  }

  if (typeof performance.setResourceTimingBufferSize === "function") {
    performance.setResourceTimingBufferSize(1024);
  }
  performance.addEventListener?.("resourcetimingbufferfull", () => {
    violations.add("resource-timing-buffer-full");
  });
  const initialSnapshotPromise = captureInitialSnapshotAfterLoad();

  observe("largest-contentful-paint", (entries) => {
    const last = entries.at(-1);
    if (last) lcpMs = last.startTime;
  });
  observe("layout-shift", (entries) => {
    for (const entry of entries) {
      if (!entry.hadRecentInput) {
        layoutShifts.push({ startTime: entry.startTime, value: entry.value });
      }
    }
  });
  observe("longtask", (entries) => {
    longTaskDurations.push(...entries.map((entry) => entry.duration));
  });
  observe(
    "event",
    (entries) => {
      interactionDurations.push(
        ...entries.filter((entry) => entry.interactionId > 0).map((entry) => entry.duration),
      );
    },
    { durationThreshold: 16 },
  );

  requestAnimationFrame(sampleFrame);
  document.addEventListener("visibilitychange", handleVisibilityChange);
  handleVisibilityChange();

  document.addEventListener(
    "click",
    (event) => {
      if (!session) return;
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (!target) return;
      const insideOverlay = event
        .composedPath()
        .some(
          (node) =>
            node instanceof Element && node.hasAttribute("data-pliego-route-overlay"),
        );
      const disclosureControl = Boolean(
        target.closest(
          "summary, [aria-expanded], [aria-haspopup], [role=menuitem], menu button",
        ),
      );
      const buttonControl = Boolean(
        target.closest(
          "button, [role=button], input[type=button], input[type=submit]",
        ),
      );
      const interactionStep = probeLogic.classifyAuthoredClick({
        insideOverlay,
        disclosureControl,
        buttonControl,
      });
      if (interactionStep) setStep(interactionStep, "complete");

      if (insideOverlay) return;
      const anchor = target.closest("a[href]");
      if (!anchor) return;
      const destination = new URL(anchor.href, location.href);
      if (destination.origin === location.origin && destination.pathname !== location.pathname) {
        setStep("navigation", "complete");
        persistState();
      }
    },
    true,
  );
  document.addEventListener(
    "toggle",
    () => setStep("disclosure", "complete"),
    true,
  );
  window.addEventListener(
    "scroll",
    () => {
      if (Math.abs(window.scrollY - (persisted?.initialScrollY ?? 0)) >= innerHeight * 0.65) {
        setStep("scroll", "complete");
      }
    },
    { passive: true },
  );
  window.addEventListener("pagehide", () => {
    pageHiding = true;
    cancelBackgroundTimer();
    if (session && !submitted) {
      persistElapsed();
      const payload = buildSegment(false);
      queuePendingSegment(payload);
      navigator.sendBeacon(
        "/_pliego/api/segments",
        new Blob([JSON.stringify(payload)], { type: "application/json" }),
      );
      submitted = true;
    }
  });
  window.addEventListener("pageshow", (event) => {
    if (!event.persisted || !session) return;
    pageHiding = false;
    cancelBackgroundTimer();
    persisted = loadState();
    submitted = false;
    resetSegmentState();
    flushPendingSegments().catch((error) => {
      violations.add(`pending-segment-flush-failed:${error.message}`);
    });
    handleVisibilityChange();
  });

  recordDevelopmentServerArtifact();

  initialize().catch((error) => {
    violations.add(`collector-error:${error.message}`);
    mountFailure(error.message);
  });

  async function initialize() {
    const response = await fetch("/_pliego/api/session", { cache: "no-store" });
    session = await response.json();
    if (!response.ok) throw new Error(session.error ?? "Route Lab session unavailable");
    storageKey = `pliego.route-lab.${session.id}`;
    persisted = loadState();
    try {
      await flushPendingSegments();
    } catch (error) {
      violations.add(`pending-segment-flush-failed:${error.message}`);
    }
    await initialSnapshotPromise;
    persisted.steps.ready = document.readyState === "complete" ? "complete" : persisted.steps.ready;
    if (
      persisted.steps.navigation === "complete" &&
      normalizePath(location.pathname) === normalizePath(session.route)
    ) {
      persisted.steps.returnToInitial = "complete";
    }
    if (document.readyState !== "complete") {
      window.addEventListener(
        "load",
        () => {
          setStep("ready", "complete");
          renderSteps();
        },
        { once: true },
      );
    }
    await domReady();
    mountPanel();
    updateClock();
    timerHandle = window.setInterval(updateClock, 250);
  }

  function loadState() {
    let stored;
    try {
      stored = JSON.parse(localStorage.getItem(storageKey) ?? "null");
    } catch {
      stored = null;
    }
    if (stored?.sessionId === session.id) {
      stored.pendingSegments ??= {};
      stored.manualSteps ??= {};
      return stored;
    }
    const value = {
      sessionId: session.id,
      runStartedAt: Date.now(),
      elapsedBeforePageMs: 0,
      initialScrollY: window.scrollY,
      manualSteps: {},
      pendingSegments: {},
      steps: {
        ready: "missed",
        scroll: "missed",
        navigation: "missed",
        visualResponse: "missed",
        disclosure: "missed",
        sceneHold: "missed",
        returnToInitial: "missed",
      },
    };
    localStorage.setItem(storageKey, JSON.stringify(value));
    return value;
  }

  function mountPanel() {
    const host = document.createElement("div");
    host.dataset.pliegoRouteOverlay = "";
    host.style.cssText = "position:fixed;right:12px;bottom:12px;z-index:2147483647";
    const shadow = host.attachShadow({ mode: "open" });
    shadow.innerHTML = `
      <style>
        *{box-sizing:border-box;letter-spacing:0}
        .panel{width:min(320px,calc(100vw - 24px));background:#111411;color:#f8f9f5;border:1px solid #515951;border-radius:6px;font:12px/1.3 Arial,sans-serif;box-shadow:0 14px 36px rgba(0,0,0,.28)}
        header{display:grid;grid-template-columns:1fr auto;gap:12px;padding:12px 12px 10px;border-bottom:1px solid #394039}
        .label{color:#70cbb8;font:9px/1.2 Consolas,monospace;text-transform:uppercase}
        strong{display:block;font-size:14px;margin-top:3px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
        .collapse{width:32px;height:32px;border:1px solid #515951;border-radius:4px;background:transparent;color:#f8f9f5;cursor:pointer}
        .body{padding:10px 12px 12px}
        .time{display:flex;align-items:baseline;justify-content:space-between;border-bottom:4px solid #d6b329;padding-bottom:8px;margin-bottom:8px}
        .time output{font:28px/1 Consolas,monospace}
        .time span{color:#aeb6ae;font:9px/1.2 Consolas,monospace}
        ol{list-style:none;margin:0;padding:0}
        li{display:grid;grid-template-columns:1fr 88px;align-items:center;gap:8px;border-bottom:1px solid #2d332d;min-height:31px}
        li span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
        li button{height:23px;border:1px solid #515951;border-radius:4px;background:#202620;color:#cbd1cb;font:9px Consolas,monospace;cursor:pointer}
        li button[data-state=complete]{border-color:#1d6b5d;color:#70cbb8}
        li button[data-state=not-applicable]{border-color:#315b87;color:#93b8e2}
        .actions{display:grid;grid-template-columns:1fr auto;gap:8px;margin-top:10px}
        .finish,.reject{min-height:38px;border:0;border-radius:4px;cursor:pointer;font-weight:700}
        .finish{background:#f8f9f5;color:#111411}
        .finish:disabled{cursor:wait;opacity:.45}
        .reject{width:72px;background:#c54a35;color:#fff}
        .status{color:#aeb6ae;font:9px/1.4 Consolas,monospace;margin:9px 0 0;overflow-wrap:anywhere}
        .panel.is-collapsed .body{display:none}
        button:focus-visible{outline:3px solid #93b8e2;outline-offset:2px}
      </style>
      <section class="panel" aria-label="PLIEGO route measurement">
        <header>
          <div><span class="label">PLIEGO / Route run</span><strong></strong></div>
          <button class="collapse" type="button" title="Collapse measurement panel" aria-label="Collapse measurement panel">-</button>
        </header>
        <div class="body">
          <div class="time"><output>00.0</output><span>10.0 s minimum</span></div>
          <ol></ol>
          <div class="actions">
            <button class="finish" type="button" disabled>Finish run</button>
            <button class="reject" type="button">Reject</button>
          </div>
          <p class="status" role="status">Recording from document load.</p>
        </div>
      </section>`;
    panel = shadow.querySelector(".panel");
    panel.querySelector("strong").textContent = `${session.targetId}${session.route}`;
    timerNode = panel.querySelector("output");
    finishButton = panel.querySelector(".finish");
    statusNode = panel.querySelector(".status");
    panel.querySelector(".collapse").addEventListener("click", () => {
      panel.classList.toggle("is-collapsed");
    });
    finishButton.addEventListener("click", () => finishRun());
    panel.querySelector(".reject").addEventListener("click", () => {
      const reason = window.prompt("Reason for rejecting this run:", "operator rejected run");
      if (!reason) return;
      violations.add(`operator-rejected:${reason.slice(0, 120)}`);
      finishRun({ operatorRejected: true });
    });
    renderSteps();
    document.documentElement.append(host);
  }

  function renderSteps() {
    if (!panel || !persisted) return;
    const list = panel.querySelector("ol");
    list.replaceChildren(
      ...Object.entries(stepLabels).map(([key, label]) => {
        const item = document.createElement("li");
        const text = document.createElement("span");
        const button = document.createElement("button");
        text.textContent = label;
        button.type = "button";
        button.dataset.state = persisted.steps[key];
        button.textContent = stateLabel(persisted.steps[key]);
        button.title =
          key === "sceneHold"
            ? "Detected automatically after the ten-second hold"
            : "Click to cycle complete, not applicable, and missed";
        button.disabled = key === "sceneHold";
        button.addEventListener("click", () => {
          const next =
            persisted.steps[key] === "missed"
              ? "complete"
              : persisted.steps[key] === "complete"
                ? "not-applicable"
                : "missed";
          setStepManually(key, next);
        });
        item.append(text, button);
        return item;
      }),
    );
  }

  function updateClock() {
    if (!persisted || !timerNode) return;
    const elapsed = Math.max(0, Date.now() - persisted.runStartedAt);
    timerNode.textContent = (elapsed / 1000).toFixed(1).padStart(4, "0");
    if (elapsed >= minimumRunDurationMs) {
      setStep(
        "sceneHold",
        probeLogic.resolveSceneHold({ hasScene: hasCanvasScene() }),
      );
    }
    if (
      persisted.steps.navigation === "complete" &&
      normalizePath(location.pathname) === normalizePath(session.route)
    ) {
      setStep("returnToInitial", "complete");
    }
    syncFinishGate(elapsed);
  }

  function syncFinishGate(elapsed = Math.max(0, Date.now() - persisted.runStartedAt)) {
    if (!finishButton || !statusNode || !recording || submitted) return;
    const gate = probeLogic.evaluateFinishGate({
      elapsedMs: elapsed,
      minimumDurationMs: minimumRunDurationMs,
      steps: persisted.steps,
    });
    finishButton.disabled = !gate.ready;
    if (!gate.minimumMet) {
      statusNode.textContent = `Recording / ${(gate.remainingMs / 1000).toFixed(1)} s until the minimum duration.`;
      return;
    }
    if (gate.missingSteps.length) {
      const missing = gate.missingSteps.map((key) => stepLabels[key] ?? key).join(", ");
      const returnHint = gate.missingSteps.includes("returnToInitial")
        ? ` Navigate back to ${session.route} before finishing.`
        : "";
      statusNode.textContent = `Finish locked / complete or mark N/A: ${missing}.${returnHint}`;
      return;
    }
    statusNode.textContent = "All required steps complete. Finish run is ready.";
  }

  async function finishRun({ operatorRejected = false } = {}) {
    if (submitted) return;
    const gate = probeLogic.evaluateFinishGate({
      elapsedMs: Math.max(0, Date.now() - persisted.runStartedAt),
      minimumDurationMs: minimumRunDurationMs,
      steps: persisted.steps,
    });
    if (!probeLogic.canSealRun({ finishReady: gate.ready, operatorRejected })) {
      syncFinishGate();
      return;
    }
    finishButton.disabled = true;
    recording = false;
    statusNode.textContent = "Measuring decoded media and sealing the receipt.";
    clearInterval(timerHandle);
    try {
      await measureImageDecode();
      persistElapsed();
      await flushPendingSegments();
      const payload = buildSegment(true);
      submitted = true;
      const response = await fetch("/_pliego/api/segments", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      const receipt = await response.json();
      if (!response.ok) throw new Error(receipt.details?.join("; ") ?? receipt.error);
      localStorage.removeItem(storageKey);
      statusNode.textContent = receipt.candidate
        ? `Candidate saved / ${receipt.fileName}`
        : `Saved with ${receipt.violations.length} review flag(s) / ${receipt.fileName}`;
      finishButton.textContent = "Open Route Lab";
      finishButton.disabled = false;
      finishButton.addEventListener(
        "click",
        () => location.assign(session.dashboardUrl),
        { once: true },
      );
    } catch (error) {
      submitted = false;
      recording = true;
      timerHandle = window.setInterval(updateClock, 250);
      syncFinishGate();
      statusNode.textContent = error instanceof Error ? error.message : String(error);
    }
  }

  function buildSegment(final) {
    recordDevelopmentServerArtifact();
    const snapshot = initialSnapshotForSegment();
    const resources = performance
      .getEntriesByType("resource")
      .slice(resourceBaseline)
      .filter((entry) => !new URL(entry.name, location.href).pathname.startsWith("/_pliego/"));
    const navigation = includeNavigation ? performance.getEntriesByType("navigation")[0] : null;
    const cachedResponseCount =
      resources.filter(isCachedResponse).length + (isCachedResponse(navigation) ? 1 : 0);
    const sceneMetrics = readSceneMetrics();
    const targetMetrics = readTargetMetrics();
    const durationMs = Math.max(1, performance.now() - segmentStartedAt);
    const path = new URL(location.href).pathname;
    return {
      segmentVersion: "1.1.0",
      capturedAt: new Date().toISOString(),
      sessionId: session.id,
      segmentId: randomHex(8),
      final,
      pagePath: path,
      durationMs,
      browserFingerprint: navigator.userAgent,
      viewport: {
        ...initialViewport,
      },
      initialSnapshot: snapshot,
      conditions: {
        foreground: probeLogic.foregroundAtCapture({
          leftForeground,
          visibilityState: document.visibilityState,
          pageHiding,
        }),
        reducedMotionMatched: matchMedia("(prefers-reduced-motion: reduce)").matches,
        serviceWorkerControlled: Boolean(navigator.serviceWorker?.controller),
      },
      steps: { ...persisted.steps },
      metrics: {
        transferBytes:
          finite(navigation?.transferSize) + sum(resources.map((entry) => entry.transferSize)),
        encodedBodyBytes:
          finite(navigation?.encodedBodySize) + sum(resources.map((entry) => entry.encodedBodySize)),
        decodedBodyBytes:
          finite(navigation?.decodedBodySize) + sum(resources.map((entry) => entry.decodedBodySize)),
        resourceCount: resources.length,
        cachedResponseCount,
        decodeDurations: [...decodeDurations],
        targetDecodeDurations: finiteArray(targetMetrics?.decodeDurations),
        longTaskDurations: [...longTaskDurations],
        targetMainThreadDurations: finiteArray(targetMetrics?.mainThreadTaskDurations),
        interactionDurations: [...interactionDurations],
        estimatedVramBytes:
          integerOrNull(sceneMetrics?.estimatedVramBytes) ?? estimateVramBytes(),
        drawCalls: integerOrNull(sceneMetrics?.drawCalls),
        triangles: integerOrNull(sceneMetrics?.triangles),
        drawCallSamples: finiteArray(sceneMetrics?.drawCallSamples),
        triangleSamples: finiteArray(sceneMetrics?.triangleSamples),
        sceneHook: Boolean(sceneMetrics),
        activeTier:
          typeof window.__PLIEGO_ACTIVE_TIER__ === "string"
            ? window.__PLIEGO_ACTIVE_TIER__
            : typeof targetMetrics?.activeTier === "string"
              ? targetMetrics.activeTier
              : null,
        webglRenderer: final
          ? sceneMetrics
            ? sceneRenderer(sceneMetrics)
            : collectWebglRenderer()
          : null,
        hasWebglCanvas: Boolean(sceneMetrics) || document.querySelector("canvas") !== null,
        frameDeltas: [...frameDeltas],
        lcpMs,
        cls: supported.has("layout-shift") ? computeClsSessionWindow(layoutShifts) : null,
      },
      capabilities: {
        lcp: supported.has("largest-contentful-paint"),
        longtask: supported.has("longtask"),
        eventTiming: supported.has("event"),
        layoutShift: supported.has("layout-shift"),
      },
      violations: [...violations],
    };
  }

  function captureInitialSnapshotAfterLoad() {
    return new Promise((resolve) => {
      const capture = () => {
        probeLogic.afterTwoAnimationFrames(requestAnimationFrame, () => {
          initialSnapshot ??= createInitialSnapshot();
          resolve(initialSnapshot);
        });
      };
      if (document.readyState === "complete") capture();
      else window.addEventListener("load", capture, { once: true });
    });
  }

  function initialSnapshotForSegment() {
    if (initialSnapshot) return initialSnapshot;
    violations.add("initial-snapshot-not-ready");
    initialSnapshot = createInitialSnapshot();
    return initialSnapshot;
  }

  function createInitialSnapshot() {
    const navigation = performance.getEntriesByType("navigation")[0];
    if (!navigation) violations.add("initial-navigation-timing-unavailable");
    const resources = performance
      .getEntriesByType("resource")
      .filter((entry) => {
        const path = probeLogic.resourcePath(entry.name, location.href);
        return !path?.startsWith("/_pliego/");
      });
    const timings = [
      ...(navigation ? [{ entryType: "navigation", timing: navigation }] : []),
      ...resources.map((timing) => ({ entryType: "resource", timing })),
    ].sort((left, right) => {
      const delta = finite(left.timing.startTime) - finite(right.timing.startTime);
      if (delta !== 0) return delta;
      return left.entryType === "navigation" ? -1 : 1;
    });
    const overflowed = timings.length > 512;
    if (overflowed) violations.add("initial-resource-ledger-overflow");
    const ledger = timings
      .slice(0, 512)
      .map(({ entryType, timing }) => Object.freeze(resourceLedgerEntry(entryType, timing)));
    const summary = summarizeLedger(ledger);
    return Object.freeze({
      capturedAt: new Date().toISOString(),
      overflowed,
      resources: Object.freeze(ledger),
      ...summary,
    });
  }

  function resourceLedgerEntry(entryType, timing) {
    let url;
    try {
      url = new URL(timing.name || location.href, location.href);
    } catch {
      url = new URL(location.pathname, location.href);
      violations.add("initial-resource-path-invalid");
    }
    const scope = url.origin === location.origin ? "target-origin" : "external";
    let path = probeLogic.resourcePath(url.href, location.href) ?? "/";
    if (!path.startsWith("/") || path.startsWith("//")) {
      path = `/${path.replace(/^\/+/, "")}`;
      violations.add("initial-resource-path-invalid");
    }
    if (path.length > 512) {
      path = path.slice(0, 512);
      violations.add("initial-resource-path-truncated");
    }
    const transferBytes = nonNegativeInteger(timing.transferSize);
    const encodedBodyBytes = nonNegativeInteger(timing.encodedBodySize);
    const decodedBodyBytes = nonNegativeInteger(timing.decodedBodySize);
    const cacheState = probeLogic.cacheStateForTiming({
      scope,
      transferBytes,
      encodedBodyBytes,
      decodedBodyBytes,
    });
    if (cacheState === "opaque") violations.add("initial-resource-cache-opaque");
    if (cacheState === "unknown") violations.add("initial-resource-cache-unknown");
    const rawInitiator = entryType === "navigation" ? "navigation" : timing.initiatorType;
    return {
      entryType,
      scope,
      path,
      initiator: String(rawInitiator || "other").slice(0, 64),
      transferBytes,
      encodedBodyBytes,
      decodedBodyBytes,
      cacheState,
      durationMs: Math.max(0, Math.round(finite(timing.duration) * 100) / 100),
    };
  }

  function summarizeLedger(entries) {
    return {
      transferBytes: sum(entries.map((entry) => entry.transferBytes)),
      encodedBodyBytes: sum(entries.map((entry) => entry.encodedBodyBytes)),
      decodedBodyBytes: sum(entries.map((entry) => entry.decodedBodyBytes)),
      resourceCount: entries.filter((entry) => entry.entryType === "resource").length,
      cachedResponseCount: entries.filter((entry) =>
        ["local-cache", "validated-cache"].includes(entry.cacheState),
      ).length,
    };
  }

  function nonNegativeInteger(value) {
    return Math.max(0, Math.round(finite(value)));
  }

  function isCachedResponse(entry) {
    return Boolean(
      entry &&
        entry.decodedBodySize > 0 &&
        (entry.transferSize === 0 || entry.encodedBodySize === 0),
    );
  }

  async function measureImageDecode() {
    const images = [...document.images].filter((image) => image.complete && image.naturalWidth > 0);
    await Promise.all(
      images.map(async (image) => {
        if (typeof image.decode !== "function") return;
        const started = performance.now();
        try {
          await image.decode();
          decodeDurations.push(performance.now() - started);
        } catch {
          violations.add(`image-decode-failed:${image.currentSrc || image.src}`);
        }
      }),
    );
  }

  function estimateVramBytes() {
    const imageBytes = [...document.images].reduce(
      (total, image) => total + image.naturalWidth * image.naturalHeight * 4,
      0,
    );
    const videoBytes = [...document.querySelectorAll("video")].reduce(
      (total, video) => total + video.videoWidth * video.videoHeight * 4,
      0,
    );
    const canvasBytes = [...document.querySelectorAll("canvas")].reduce(
      (total, canvas) => total + canvas.width * canvas.height * 4,
      0,
    );
    return Math.round(imageBytes + videoBytes + canvasBytes);
  }

  function readSceneMetrics() {
    const hook = window.__PLIEGO_SCENE_METRICS__;
    try {
      const value = typeof hook === "function" ? hook() : hook;
      return value && typeof value === "object" ? value : null;
    } catch (error) {
      violations.add(`scene-hook-error:${error.message}`);
      return null;
    }
  }

  function readTargetMetrics() {
    const hook = window.__PLIEGO_PERFORMANCE_METRICS__;
    try {
      const value = typeof hook === "function" ? hook() : hook;
      return value && typeof value === "object" ? value : null;
    } catch (error) {
      violations.add(`performance-hook-error:${error.message}`);
      return null;
    }
  }

  function sceneRenderer(metrics) {
    const value = metrics.renderer ?? metrics.rendererFingerprint;
    return typeof value === "string" && value ? value : null;
  }

  function persistElapsed() {
    if (!persisted) return;
    persisted.elapsedBeforePageMs += Math.max(0, performance.now() - segmentStartedAt);
    persistState();
  }

  function queuePendingSegment(payload) {
    if (!persisted) return;
    persisted.pendingSegments[payload.segmentId] = payload;
    persistState();
  }

  async function flushPendingSegments() {
    if (!persisted) return;
    for (const [segmentId, payload] of Object.entries(persisted.pendingSegments)) {
      const response = await fetch("/_pliego/api/segments", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      if (!response.ok) {
        const detail = await response.text();
        throw new Error(`segment ${segmentId} was not accepted: ${detail.slice(0, 160)}`);
      }
      delete persisted.pendingSegments[segmentId];
      persistState();
    }
  }

  function handleVisibilityChange() {
    cancelBackgroundTimer();
    if (pageHiding || document.visibilityState === "visible") return;
    backgroundTimer = window.setTimeout(() => {
      backgroundTimer = undefined;
      if (pageHiding || document.visibilityState === "visible") return;
      leftForeground = true;
      violations.add("document-left-foreground");
    }, backgroundConfirmationMs);
  }

  function cancelBackgroundTimer() {
    if (backgroundTimer === undefined) return;
    window.clearTimeout(backgroundTimer);
    backgroundTimer = undefined;
  }

  function hasCanvasScene() {
    const hook = window.__PLIEGO_SCENE_METRICS__;
    return Boolean(
      document.querySelector("canvas") ||
        typeof hook === "function" ||
        (hook && typeof hook === "object"),
    );
  }

  function recordDevelopmentServerArtifact() {
    const developmentServer = probeLogic.hasDevelopmentServerArtifact({
      scriptSources: [...document.scripts]
        .map((script) => script.src || script.getAttribute("src") || "")
        .filter(Boolean),
      hasAstroToolbar: document.querySelector("astro-dev-toolbar") !== null,
      hasViteOverlay: document.querySelector("vite-error-overlay") !== null,
      hasViteStyle: document.querySelector("style[data-vite-dev-id]") !== null,
    });
    if (developmentServer) violations.add("development-server-artifact");
  }

  function resetSegmentState() {
    segmentStartedAt = performance.now();
    resourceBaseline = performance.getEntriesByType("resource").length;
    includeNavigation = false;
    recording = true;
    previousFrame = undefined;
    frameDeltas.length = 0;
    longTaskDurations.length = 0;
    interactionDurations.length = 0;
    decodeDurations.length = 0;
    lcpMs = null;
    layoutShifts.length = 0;
    leftForeground = false;
    pageHiding = false;
    cancelBackgroundTimer();
    handleVisibilityChange();
  }

  function setStep(key, value) {
    if (!persisted || persisted.steps[key] === value) return;
    persisted.steps[key] = value;
    persistState();
    renderSteps();
  }

  function setStepManually(key, value) {
    if (!persisted) return;
    persisted.manualSteps[key] = true;
    if (persisted.steps[key] === value) {
      persistState();
      return;
    }
    setStep(key, value);
  }

  function persistState() {
    if (!storageKey || !persisted) return;
    try {
      localStorage.setItem(storageKey, JSON.stringify(persisted));
    } catch {
      violations.add("local-state-write-failed");
    }
  }

  function sampleFrame(now) {
    if (recording && previousFrame !== undefined) frameDeltas.push(now - previousFrame);
    previousFrame = now;
    requestAnimationFrame(sampleFrame);
  }

  function collectWebglRenderer() {
    const canvas = document.createElement("canvas");
    const context = canvas.getContext("webgl2") ?? canvas.getContext("webgl");
    if (!context) return null;
    const debug = context.getExtension("WEBGL_debug_renderer_info");
    const renderer = debug
      ? context.getParameter(debug.UNMASKED_RENDERER_WEBGL)
      : context.getParameter(context.RENDERER);
    context.getExtension("WEBGL_lose_context")?.loseContext();
    return typeof renderer === "string" ? renderer : null;
  }

  function observe(type, callback, options = {}) {
    if (!supported.has(type)) return;
    try {
      const observer = new PerformanceObserver((list) => callback(list.getEntries()));
      observer.observe({ type, buffered: true, ...options });
    } catch (error) {
      violations.add(`observer-error:${type}:${error.message}`);
    }
  }

  function mountFailure(message) {
    const node = document.createElement("p");
    node.textContent = `PLIEGO Route Lab stopped: ${message}`;
    node.style.cssText =
      "position:fixed;right:12px;bottom:12px;z-index:2147483647;max-width:320px;margin:0;padding:14px;background:#c54a35;color:white;font:12px Arial,sans-serif;border-radius:6px";
    document.documentElement.append(node);
  }

  function stateLabel(state) {
    if (state === "complete") return "COMPLETE";
    if (state === "not-applicable") return "N/A";
    return "MISSED";
  }

  function domReady() {
    if (document.readyState !== "loading") return Promise.resolve();
    return new Promise((resolve) =>
      document.addEventListener("DOMContentLoaded", resolve, { once: true }),
    );
  }

  function normalizePath(value) {
    return value.replace(/\/$/, "") || "/";
  }

  function randomHex(bytes) {
    const data = new Uint8Array(bytes);
    crypto.getRandomValues(data);
    return [...data].map((value) => value.toString(16).padStart(2, "0")).join("");
  }

  function finite(value) {
    return Number.isFinite(value) ? Number(value) : 0;
  }

  function sum(values) {
    return values.reduce((total, value) => total + finite(value), 0);
  }

  function integerOrNull(value) {
    return Number.isFinite(value) ? Math.max(0, Math.round(value)) : null;
  }

  function finiteArray(values) {
    return Array.isArray(values) ? values.filter(Number.isFinite).map(Number) : [];
  }

  function computeClsSessionWindow(entries) {
    let maximum = 0;
    let windowValue = 0;
    let windowStart = 0;
    let previous = 0;
    for (const entry of entries) {
      if (
        windowValue === 0 ||
        entry.startTime - previous > 1_000 ||
        entry.startTime - windowStart > 5_000
      ) {
        windowValue = entry.value;
        windowStart = entry.startTime;
      } else {
        windowValue += entry.value;
      }
      previous = entry.startTime;
      maximum = Math.max(maximum, windowValue);
    }
    return maximum;
  }
})();
