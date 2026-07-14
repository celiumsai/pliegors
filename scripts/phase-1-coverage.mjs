export const motionModes = ["default", "reduced"];

export function enumerateRequiredCases(plan) {
  const cases = [];
  for (const device of plan.devices.filter((candidate) => candidate.required)) {
    const orientations = [
      device.viewport.orientation,
      ...(device.viewport.additionalOrientations ?? []),
    ];
    for (const target of plan.targets) {
      for (const route of target.routes) {
        for (const tier of device.tiers) {
          for (const orientation of orientations) {
            for (const cacheMode of plan.policy.cacheModes) {
              for (const motionMode of motionModes) {
                cases.push({
                  deviceId: device.id,
                  targetId: target.id,
                  route,
                  tier,
                  orientation,
                  cacheMode,
                  motionMode,
                });
              }
            }
          }
        }
      }
    }
  }
  return cases;
}

export function measurementCaseKey(value) {
  return [
    value.deviceId,
    value.targetId,
    value.route,
    value.tier,
    value.orientation,
    value.cacheMode,
    value.motionMode,
  ].join("|");
}

export function evaluateBudgets(report, budgets) {
  const failures = [];
  const observations = report.observations;
  if (
    report.tier === "universal" &&
    observations.transferBytes > budgets.universalFirstViewportTransferBytes
  ) {
    failures.push(
      `transferBytes ${observations.transferBytes} > ${budgets.universalFirstViewportTransferBytes}`,
    );
  }
  if (
    report.tier === "lite" &&
    observations.estimatedVramBytes > budgets.liteEstimatedVramBytes
  ) {
    failures.push(
      `estimatedVramBytes ${observations.estimatedVramBytes} > ${budgets.liteEstimatedVramBytes}`,
    );
  }
  if (report.tier === "lite" && observations.frameP95Ms > budgets.liteFrameP95Ms) {
    failures.push(`frameP95Ms ${observations.frameP95Ms} > ${budgets.liteFrameP95Ms}`);
  }
  if (observations.lcpP75Ms > budgets.lcpP75Ms) {
    failures.push(`lcpP75Ms ${observations.lcpP75Ms} > ${budgets.lcpP75Ms}`);
  }
  if (observations.inpP75Ms > budgets.inpP75Ms) {
    failures.push(`inpP75Ms ${observations.inpP75Ms} > ${budgets.inpP75Ms}`);
  }
  if (observations.clsP75 > budgets.clsP75) {
    failures.push(`clsP75 ${observations.clsP75} > ${budgets.clsP75}`);
  }
  return failures;
}

export function aggregateAcceptedRuns(runs) {
  if (!runs.length) throw new Error("at least one accepted run is required");
  const values = (field) => {
    const result = runs.map((run) => run.observations[field]);
    if (result.some((value) => !Number.isFinite(value))) {
      throw new Error(`${field} is unavailable in one or more accepted runs`);
    }
    return result;
  };
  return {
    transferBytes: Math.round(percentile(values("transferBytes"), 0.5)),
    decodeP95Ms: percentile(values("decodeP95Ms"), 0.95),
    mainThreadP95Ms: percentile(values("mainThreadP95Ms"), 0.95),
    estimatedVramBytes: Math.round(Math.max(...values("estimatedVramBytes"))),
    drawCalls: Math.round(percentile(values("drawCalls"), 0.95)),
    triangles: Math.round(percentile(values("triangles"), 0.95)),
    frameP95Ms: percentile(values("frameP95Ms"), 0.95),
    lcpP75Ms: percentile(values("lcpMs"), 0.75),
    inpP75Ms: percentile(values("inpMs"), 0.75),
    clsP75: percentile(values("cls"), 0.75),
  };
}

export function percentile(values, quantile) {
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * quantile) - 1);
  return Math.round(sorted[index] * 100) / 100;
}
