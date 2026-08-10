#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-only

import { execFileSync, spawn as spawnChild, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const samples = integerOption("--samples", 5, 1, 30);
const output = path.resolve(
  root,
  valueAfter("--output") ?? "target/evidence/g4-engineering-readiness.json",
);
const accept = process.argv.includes("--accept");
const keep = process.argv.includes("--keep");
const target = process.env.PLIEGO_G4_CLI_TARGET
  ?? path.join(os.tmpdir(), "pliegors-g4-engineering-cli");
const cli = process.env.PLIEGO_G4_CLI
  ?? path.join(target, "release", process.platform === "win32" ? "pliego.exe" : "pliego");
const revision = text("git", ["rev-parse", "--verify", "HEAD"], root);
const dirty = text("git", ["status", "--porcelain"], root).length > 0;

if (accept && dirty) {
  throw new Error("--accept requires a clean exact Git revision");
}
if (accept && samples < 5) {
  throw new Error("--accept requires at least five independent samples");
}
if (accept && process.env.PLIEGO_G4_CLI) {
  throw new Error("--accept builds the release CLI from the measured revision");
}

const workspace = await mkdtemp(path.join(os.tmpdir(), "pliegors-g4-measure-"));
const validationTarget = path.join(workspace, "validation-target");
const observations = [];
const canMeasurePeakRss = process.platform === "linux";

try {
  if (!process.env.PLIEGO_G4_CLI) {
    run("cargo", [
      "build",
      "--manifest-path", path.join(root, "Cargo.toml"),
      "-p", "pliego-cli",
      "--release",
      "--locked",
    ], root, { CARGO_TARGET_DIR: target });
  }

  for (let sample = 1; sample <= samples; sample += 1) {
    const sampleRoot = path.join(workspace, `sample-${sample}`);
    const project = path.join(sampleRoot, "application");
    const cargoTarget = path.join(sampleRoot, "cargo-target");
    const environment = { CARGO_TARGET_DIR: cargoTarget };
    const validationEnvironment = { CARGO_TARGET_DIR: validationTarget };
    run(cli, ["new", project, "--framework-path", root], root);
    run(cli, ["check"], project, validationEnvironment);
    run("cargo", ["test", "--locked", "--quiet"], project, validationEnvironment);
    assert(
      !(await pathExists(cargoTarget)),
      `sample ${sample}: measured cold Cargo target was not empty`,
    );

    const cold = await measured(cli, ["build"], project, environment);
    const coldRecord = cacheStatus(project, environment);
    assertRecord(coldRecord, "executed", "cold", sample);
    assert(coldRecord.reusedArtifacts === 0, `sample ${sample}: cold build reused artifacts`);

    const noOp = await measured(cli, ["build"], project, environment);
    const noOpRecord = cacheStatus(project, environment);
    assertRecord(noOpRecord, "no-op", "no-op", sample);
    assert(noOpRecord.renderedArtifacts === 0, `sample ${sample}: no-op rendered artifacts`);
    assert(noOpRecord.receiptAfter === coldRecord.receiptAfter, `sample ${sample}: no-op receipt drifted`);
    assert(noOp.humanNoOpVisible, `sample ${sample}: no-op was not visible in human output`);

    const domain = path.join(project, "src", "domain.rs");
    await writeFile(domain, `${await readFile(domain, "utf8")}\n// G4 content sample ${sample}\n`);
    const content = await measured(cli, ["build"], project, environment);
    const contentRecord = cacheStatus(project, environment);
    assertRecord(contentRecord, "executed", "content change", sample);
    assert(
      equal(contentRecord.changedSources, ["src/domain.rs"]),
      `sample ${sample}: content invalidation was not scoped`,
    );
    assert(
      contentRecord.reusedArtifacts >= 2 && contentRecord.renderedArtifacts > 0,
      `sample ${sample}: content change did not selectively reuse routes`,
    );

    const css = path.join(project, "assets", "site.css");
    await writeFile(css, `${await readFile(css, "utf8")}\n/* G4 asset sample ${sample} */\n`);
    const asset = await measured(cli, ["build"], project, environment);
    const assetRecord = cacheStatus(project, environment);
    assertRecord(assetRecord, "executed", "asset change", sample);
    assert(
      equal(assetRecord.changedSources, ["assets/site.css"]),
      `sample ${sample}: asset invalidation was not scoped`,
    );
    assert(
      assetRecord.reusedArtifacts >= 3 && assetRecord.renderedArtifacts > 0,
      `sample ${sample}: asset change did not reuse lazy routes`,
    );

    const recordPath = path.join(project, "target", ".pliego", "last-build.json");
    await writeFile(recordPath, "{}\n");
    const rejectStarted = process.hrtime.bigint();
    const rejection = spawn(cli, ["cache", "status", "--format", "json"], project, environment);
    const rejectionMs = elapsedMs(rejectStarted);
    assert(rejection.status !== 0, `sample ${sample}: corrupt cache record was accepted`);

    const recovery = await measured(cli, ["build"], project, environment);
    const recoveryRecord = cacheStatus(project, environment);
    assertRecord(recoveryRecord, "no-op", "cache recovery", sample);
    assert(
      recoveryRecord.receiptAfter === assetRecord.receiptAfter,
      `sample ${sample}: recovery changed the publication`,
    );
    run(cli, ["inspect"], project, environment);

    const ledger = JSON.parse(
      await readFile(path.join(project, "target", "site", "pliego.build.json"), "utf8"),
    );
    observations.push({
      sample,
      fixture: "official-default-revision-3",
      routes: 3,
      publicationFiles: ledger.receipt.outputs.fileCount,
      cold: observation(cold, coldRecord),
      noOp: observation(noOp, noOpRecord),
      contentChange: observation(content, contentRecord),
      assetChange: observation(asset, assetRecord),
      corruptCache: {
        rejectionDurationMs: rejectionMs,
        rejected: true,
        recovery: observation(recovery, recoveryRecord),
      },
    });
    process.stdout.write(`G4 measurement sample ${sample}/${samples} PASS\n`);
  }

  const releaseCli = await readFile(cli);
  const report = {
    contract: "pliegors-g4-incremental-measurement/1",
    revision,
    cleanRevision: !dirty,
    accepted: accept,
    platform: {
      os: os.platform(),
      architecture: os.arch(),
      release: os.release(),
      cpu: os.cpus()[0]?.model ?? "unknown",
      logicalCpus: os.cpus().length,
      totalMemoryBytes: os.totalmem(),
    },
    toolchain: {
      rustc: text("rustc", ["--version"], root),
      cargo: text("cargo", ["--version"], root),
      node: process.version,
      releaseCliBytes: releaseCli.length,
      releaseCliSha256: createHash("sha256").update(releaseCli).digest("hex"),
    },
    method: {
      samples,
      independentProjects: true,
      releaseCliBuiltOutsideTimedRegion: true,
      freshCargoTargetPerColdSample: true,
      peakRss: canMeasurePeakRss
        ? "10 ms procfs samples summed across the command process tree"
        : "unavailable",
      percentiles: "nearest-rank",
      scenarios: [
        "cold build from an empty per-sample Cargo target",
        "verified warm no-op",
        "src/domain.rs-only change",
        "assets/site.css-only change",
        "malformed private build-record rejection and verified no-op recovery",
      ],
    },
    summaries: {
      cold: summarize(observations.map((item) => item.cold)),
      noOp: summarize(observations.map((item) => item.noOp)),
      contentChange: summarize(observations.map((item) => item.contentChange)),
      assetChange: summarize(observations.map((item) => item.assetChange)),
      recovery: summarize(observations.map((item) => item.corruptCache.recovery)),
    },
    observations,
    limitations: [
      "This report describes only the named revision, host, toolchain, and official default fixture.",
      "Cold measurements include application dependency compilation but exclude release CLI compilation.",
      "No result is a competitor comparison, device guarantee, hosted CI result, or external adoption evidence.",
    ],
  };
  await mkdir(path.dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(`G4 measurement evidence: ${output}\n`);
  process.stdout.write(`G4 measurement ${accept ? "ACCEPTED" : "SMOKE"} PASS\n`);
} finally {
  if (keep) process.stdout.write(`G4 measurement workspace retained: ${workspace}\n`);
  else await rm(workspace, { recursive: true, force: true });
}

function cacheStatus(project, environment) {
  return JSON.parse(run(cli, ["cache", "status", "--format", "json"], project, environment));
}

async function measured(command, args, cwd, environment) {
  const started = process.hrtime.bigint();
  const result = await spawnMeasured(command, args, cwd, environment);
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed (${result.status})\n${result.stderr || result.stdout}`,
    );
  }
  return {
    durationMs: elapsedMs(started),
    peakRssKiB: result.peakRssKiB,
    humanNoOpVisible: result.stdout.includes("[no-op "),
  };
}

async function spawnMeasured(command, args, cwd, extraEnvironment) {
  const child = spawnChild(command, args, {
    cwd,
    env: { ...process.env, ...extraEnvironment },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const stdout = [];
  const stderr = [];
  let stdoutBytes = 0;
  let stderrBytes = 0;
  let overflow = null;
  const collect = (chunks, stream) => (chunk) => {
    const next = stream === "stdout"
      ? (stdoutBytes += chunk.length)
      : (stderrBytes += chunk.length);
    if (next > 32 * 1024 * 1024) {
      overflow = `${stream} exceeded 32 MiB`;
      child.kill();
      return;
    }
    chunks.push(chunk);
  };
  child.stdout.on("data", collect(stdout, "stdout"));
  child.stderr.on("data", collect(stderr, "stderr"));

  let peakRssKiB = canMeasurePeakRss ? 0 : null;
  let sampling = null;
  const sample = () => {
    if (!canMeasurePeakRss || sampling) return;
    sampling = processTreeRssKiB(child.pid)
      .then((rss) => {
        peakRssKiB = Math.max(peakRssKiB, rss);
      })
      .finally(() => {
        sampling = null;
      });
  };
  sample();
  const interval = canMeasurePeakRss ? setInterval(sample, 10) : null;
  const status = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  if (interval) clearInterval(interval);
  if (sampling) await sampling;
  if (overflow) throw new Error(overflow);
  if (canMeasurePeakRss) {
    assert(peakRssKiB > 0, "procfs did not observe command memory");
  }
  return {
    status,
    stdout: Buffer.concat(stdout).toString("utf8"),
    stderr: Buffer.concat(stderr).toString("utf8"),
    peakRssKiB,
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
  const own = Number.parseInt(/^VmRSS:\s+(\d+)\s+kB$/m.exec(status)?.[1] ?? "0", 10);
  const childPids = children
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map(Number);
  const descendants = await Promise.all(
    childPids.map((childPid) => processTreeRssKiB(childPid, seen)),
  );
  return own + descendants.reduce((sum, rss) => sum + rss, 0);
}

async function pathExists(targetPath) {
  try {
    await stat(targetPath);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function observation(measurement, record) {
  return {
    ...measurement,
    outcome: record.outcome,
    changedSources: record.changedSources,
    renderedArtifacts: record.renderedArtifacts,
    reusedArtifacts: record.reusedArtifacts,
    receiptBefore: record.receiptBefore,
    receiptAfter: record.receiptAfter,
    phasesMicros: record.phases,
  };
}

function summarize(items) {
  const durations = items.map((item) => item.durationMs);
  const rss = items.map((item) => item.peakRssKiB).filter(Number.isSafeInteger);
  return {
    durationMs: {
      raw: durations,
      p50: nearestRank(durations, 0.5),
      p95: nearestRank(durations, 0.95),
    },
    peakRssKiB: rss.length === items.length
      ? { raw: rss, p50: nearestRank(rss, 0.5), p95: nearestRank(rss, 0.95) }
      : null,
  };
}

function nearestRank(values, percentile) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil(sorted.length * percentile) - 1)];
}

function assertRecord(record, outcome, label, sample) {
  assert(record.recordVersion === "pliego-build/1", `sample ${sample}: ${label} record version`);
  assert(record.outcome === outcome, `sample ${sample}: ${label} outcome was ${record.outcome}`);
  assert(
    record.renderedArtifacts + record.reusedArtifacts > 0,
    `sample ${sample}: ${label} recorded no artifacts`,
  );
}

function run(command, args, cwd, extraEnvironment = {}) {
  const result = spawn(command, args, cwd, extraEnvironment);
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed (${result.status})\n${result.stderr || result.stdout}`,
    );
  }
  return result.stdout;
}

function spawn(command, args, cwd, extraEnvironment = {}) {
  return spawnSync(command, args, {
    cwd,
    env: { ...process.env, ...extraEnvironment },
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
}

function text(command, args, cwd) {
  return execFileSync(command, args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
  }).trim();
}

function elapsedMs(started) {
  return Number(process.hrtime.bigint() - started) / 1_000_000;
}

function equal(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function integerOption(option, fallback, minimum, maximum) {
  const value = valueAfter(option);
  if (value === null) return fallback;
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < minimum || parsed > maximum) {
    throw new Error(`${option} must be an integer from ${minimum} to ${maximum}`);
  }
  return parsed;
}

function valueAfter(option) {
  const direct = process.argv.find((argument) => argument.startsWith(`${option}=`));
  if (direct) return direct.slice(option.length + 1);
  const index = process.argv.indexOf(option);
  if (index < 0) return null;
  const value = process.argv[index + 1];
  if (!value || value.startsWith("--")) throw new Error(`${option} requires a value`);
  return value;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
