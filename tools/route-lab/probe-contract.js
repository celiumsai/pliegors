(() => {
  const requiredStepKeys = Object.freeze([
    "ready",
    "scroll",
    "navigation",
    "visualResponse",
    "disclosure",
    "sceneHold",
    "returnToInitial",
  ]);

  function afterTwoAnimationFrames(requestFrame, callback) {
    requestFrame(() => requestFrame(callback));
  }

  function evaluateFinishGate({ elapsedMs, minimumDurationMs, steps }) {
    const elapsed = Number.isFinite(elapsedMs) ? Math.max(0, elapsedMs) : 0;
    const minimum = Number.isFinite(minimumDurationMs)
      ? Math.max(0, minimumDurationMs)
      : 10_000;
    const missingSteps = requiredStepKeys.filter(
      (key) => !["complete", "not-applicable"].includes(steps?.[key]),
    );
    const minimumMet = elapsed >= minimum;
    return {
      ready: minimumMet && missingSteps.length === 0,
      minimumMet,
      remainingMs: Math.max(0, minimum - elapsed),
      missingSteps,
    };
  }

  function canSealRun({ finishReady, operatorRejected }) {
    return Boolean(finishReady || operatorRejected);
  }

  function classifyAuthoredClick({ insideOverlay, disclosureControl, buttonControl }) {
    if (insideOverlay) return null;
    if (disclosureControl) return "disclosure";
    if (buttonControl) return "visualResponse";
    return null;
  }

  function resolveSceneHold({ hasScene }) {
    return hasScene ? "complete" : "not-applicable";
  }

  function foregroundAtCapture({ leftForeground, visibilityState, pageHiding }) {
    return !leftForeground && (visibilityState === "visible" || pageHiding);
  }

  function cacheStateForTiming({
    scope,
    transferBytes,
    encodedBodyBytes,
    decodedBodyBytes,
  }) {
    if (transferBytes > 0 && encodedBodyBytes === 0 && decodedBodyBytes > 0) {
      return "validated-cache";
    }
    if (transferBytes > 0) return "network";
    if (decodedBodyBytes > 0) return "local-cache";
    if (
      scope === "external" &&
      transferBytes === 0 &&
      encodedBodyBytes === 0 &&
      decodedBodyBytes === 0
    ) {
      return "opaque";
    }
    return "unknown";
  }

  function resourcePath(value, baseUrl) {
    try {
      return new URL(value, baseUrl).pathname;
    } catch {
      return null;
    }
  }

  function hasDevelopmentServerArtifact({
    scriptSources = [],
    hasAstroToolbar = false,
    hasViteOverlay = false,
    hasViteStyle = false,
  }) {
    const developmentScript = scriptSources.some((source) => {
      try {
        const pathname = new URL(source, "http://route-lab.invalid").pathname;
        return pathname === "/@vite/client" || pathname.startsWith("/@react-refresh");
      } catch {
        return false;
      }
    });
    return developmentScript || hasAstroToolbar || hasViteOverlay || hasViteStyle;
  }

  Object.defineProperty(globalThis, "__PLIEGO_ROUTE_PROBE_LOGIC__", {
    configurable: false,
    enumerable: false,
    value: Object.freeze({
      afterTwoAnimationFrames,
      canSealRun,
      classifyAuthoredClick,
      cacheStateForTiming,
      evaluateFinishGate,
      foregroundAtCapture,
      hasDevelopmentServerArtifact,
      resourcePath,
      resolveSceneHold,
    }),
    writable: false,
  });
})();
