// SPDX-License-Identifier: GPL-3.0-only

import { readFile } from "node:fs/promises";

export function createProcessTreeRssSampler(
  pid,
  { intervalMs = 10, platform = process.platform } = {},
) {
  if (platform !== "linux") {
    return {
      supported: false,
      async snapshotBytes() { return null; },
      async stop() { return null; },
    };
  }

  let peakKiB = 0;
  let pending = null;
  let stopped = false;
  const sample = () => {
    if (stopped || pending) return;
    pending = processTreeRssKiB(pid)
      .then((rss) => { peakKiB = Math.max(peakKiB, rss); })
      .finally(() => { pending = null; });
  };
  sample();
  const interval = setInterval(sample, intervalMs);

  return {
    supported: true,
    async snapshotBytes() {
      sample();
      if (pending) await pending;
      return peakKiB > 0 ? peakKiB * 1024 : null;
    },
    async stop() {
      if (!stopped) {
        stopped = true;
        clearInterval(interval);
      }
      if (pending) await pending;
      return peakKiB > 0 ? peakKiB * 1024 : null;
    },
  };
}

async function processTreeRssKiB(pid, seen = new Set()) {
  if (!Number.isSafeInteger(pid) || pid <= 0 || seen.has(pid)) return 0;
  seen.add(pid);
  let status;
  let children;
  try {
    [status, children] = await Promise.all([
      readFile(`/proc/${pid}/status`, "utf8"),
      readFile(`/proc/${pid}/task/${pid}/children`, "utf8"),
    ]);
  } catch {
    return 0;
  }
  const own = Number.parseInt(/^VmRSS:\s+(\d+)\s+kB$/mu.exec(status)?.[1] ?? "0", 10);
  const descendants = await Promise.all(
    children
      .trim()
      .split(/\s+/u)
      .filter(Boolean)
      .map(Number)
      .map((child) => processTreeRssKiB(child, seen)),
  );
  return own + descendants.reduce((sum, rss) => sum + rss, 0);
}
