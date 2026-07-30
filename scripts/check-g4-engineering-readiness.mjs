#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { execFileSync, spawnSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outputArgument = valueAfter("--output");
const keep = process.argv.includes("--keep");
const target = process.env.PLIEGO_G4_CLI_TARGET
  ?? path.join(os.tmpdir(), "pliegors-g4-engineering-cli");
const cli = process.env.PLIEGO_G4_CLI
  ?? path.join(target, "release", process.platform === "win32" ? "pliego.exe" : "pliego");
const workspace = await mkdtemp(path.join(os.tmpdir(), "pliegors-g4-engineering-"));
const sharedTarget = path.join(workspace, "cargo-target");
const templates = ["default", "minimal", "editorial", "cinematic"];
const results = [];

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

  for (const template of templates) {
    const project = path.join(workspace, template);
    run(cli, [
      "new", project,
      "--template", template,
      "--framework-path", root,
    ], root);
    run(cli, ["check"], project, { CARGO_TARGET_DIR: sharedTarget });
    run("cargo", ["test", "--locked", "--quiet"], project, {
      CARGO_TARGET_DIR: sharedTarget,
    });

    const firstOutput = run(cli, ["build"], project, {
      CARGO_TARGET_DIR: sharedTarget,
    });
    const first = cacheStatus(cli, project, sharedTarget);
    assert(first.outcome === "executed", `${template}: first build was not executed`);
    assert(
      first.renderedArtifacts + first.reusedArtifacts > 0,
      `${template}: first build recorded no artifacts`,
    );

    const secondOutput = run(cli, ["build"], project, {
      CARGO_TARGET_DIR: sharedTarget,
    });
    const second = cacheStatus(cli, project, sharedTarget);
    assert(second.outcome === "no-op", `${template}: unchanged build was not a no-op`);
    assert(second.renderedArtifacts === 0, `${template}: no-op rendered artifacts`);
    assert(
      second.reusedArtifacts === first.renderedArtifacts + first.reusedArtifacts,
      `${template}: no-op count does not cover the verified graph`,
    );
    assert(
      second.receiptAfter === first.receiptAfter,
      `${template}: no-op changed the receipt`,
    );
    assert(secondOutput.includes("[no-op "), `${template}: no-op was not visible to humans`);

    const css = path.join(project, "assets", "site.css");
    await writeFile(css, `${await readFile(css, "utf8")}\n/* G4 asset invalidation */\n`);
    run(cli, ["build"], project, { CARGO_TARGET_DIR: sharedTarget });
    const assetChange = cacheStatus(cli, project, sharedTarget);
    assert(assetChange.outcome === "executed", `${template}: asset change did not execute`);
    assert(
      assetChange.changedSources.includes("assets/site.css"),
      `${template}: asset source was not reported`,
    );
    assert(
      assetChange.reusedArtifacts >= (template === "default" ? 3 : 2),
      `${template}: unaffected lazy routes were not reused`,
    );
    assert(
      assetChange.renderedArtifacts > 0,
      `${template}: changed asset was not rendered`,
    );

    let selective = null;
    if (template === "default") {
      const domain = path.join(project, "src", "domain.rs");
      await writeFile(domain, `${await readFile(domain, "utf8")}\n// G4 route invalidation\n`);
      run(cli, ["build"], project, { CARGO_TARGET_DIR: sharedTarget });
      selective = cacheStatus(cli, project, sharedTarget);
      assert(selective.outcome === "executed", "default: domain change did not execute");
      assert(
        selective.changedSources.includes("src/domain.rs"),
        "default: domain source was not reported",
      );
      assert(
        selective.reusedArtifacts >= 2 && selective.renderedArtifacts > 0,
        "default: selective route invalidation did not reuse unaffected routes",
      );
    }

    run(cli, ["inspect"], project, { CARGO_TARGET_DIR: sharedTarget });
    run(cli, ["why", "artifact", "/"], project, { CARGO_TARGET_DIR: sharedTarget });
    const recordPath = path.join(project, "target", ".pliego", "last-build.json");
    await writeFile(recordPath, "{}\n");
    expectFailure(cli, ["cache", "status", "--format", "json"], project, {
      CARGO_TARGET_DIR: sharedTarget,
    });
    run(cli, ["build"], project, { CARGO_TARGET_DIR: sharedTarget });
    const recovered = cacheStatus(cli, project, sharedTarget);
    assert(recovered.outcome === "no-op", `${template}: cache record did not recover`);
    run(cli, ["cache", "clean"], project, { CARGO_TARGET_DIR: sharedTarget });
    await assertMissing(recordPath, `${template}: cache clean left the build record`);
    run(cli, ["inspect"], project, { CARGO_TARGET_DIR: sharedTarget });

    const ledger = JSON.parse(
      await readFile(path.join(project, "target", "site", "pliego.build.json"), "utf8"),
    );
    results.push({
      template,
      receiptSha256: ledger.receiptSha256,
      artifacts: ledger.receipt.outputs.fileCount,
      first: summary(first),
      noOp: summary(second),
      assetChange: summary(assetChange),
      selectiveRouteChange: selective ? summary(selective) : null,
      cacheCorruptionRejected: true,
      cacheCleanPreservedPublication: true,
      firstOutputVisible: firstOutput.includes("PLIEGO build:"),
    });
    process.stdout.write(`G4 starter PASS ${template}\n`);
  }

  run("node", [
    path.join(root, "scripts", "check-starter-builds.mjs"),
    ...templates.map((template) => path.join(workspace, template)),
  ], root);

  const evidence = {
    contract: "pliegors-g4-engineering-readiness/1",
    sourceRevision: text("git", ["rev-parse", "--verify", "HEAD"], root),
    sourceDirty: text("git", ["status", "--porcelain"], root).length > 0,
    platform: `${os.platform()} ${os.arch()} ${os.release()}`,
    rustc: text("rustc", ["--version"], root),
    node: process.version,
    templates: results,
  };
  if (outputArgument) {
    const output = path.resolve(root, outputArgument);
    await mkdir(path.dirname(output), { recursive: true });
    await writeFile(output, `${JSON.stringify(evidence, null, 2)}\n`);
    process.stdout.write(`G4 evidence: ${output}\n`);
  }
  process.stdout.write("G4 engineering readiness PASS\n");
} finally {
  if (keep) process.stdout.write(`G4 workspace retained: ${workspace}\n`);
  else await rm(workspace, { recursive: true, force: true });
}

function run(command, args, cwd, extraEnvironment = {}) {
  return execFileSync(command, args, {
    cwd,
    env: { ...process.env, ...extraEnvironment },
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
}

function text(command, args, cwd) {
  return run(command, args, cwd).trim();
}

function expectFailure(command, args, cwd, extraEnvironment) {
  const result = spawnSync(command, args, {
    cwd,
    env: { ...process.env, ...extraEnvironment },
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
  assert(result.status !== 0, `${command} ${args.join(" ")} unexpectedly succeeded`);
}

function cacheStatus(command, project, cargoTarget) {
  return JSON.parse(run(command, ["cache", "status", "--format", "json"], project, {
    CARGO_TARGET_DIR: cargoTarget,
  }));
}

function summary(record) {
  return {
    outcome: record.outcome,
    changedSources: record.changedSources,
    renderedArtifacts: record.renderedArtifacts,
    reusedArtifacts: record.reusedArtifacts,
    totalMicros: record.phases.totalMicros,
  };
}

async function assertMissing(targetPath, message) {
  try {
    await readFile(targetPath);
  } catch (error) {
    if (error?.code === "ENOENT") return;
    throw error;
  }
  throw new Error(message);
}

function valueAfter(option) {
  const index = process.argv.indexOf(option);
  if (index < 0) return null;
  const value = process.argv[index + 1];
  if (!value || value.startsWith("--")) throw new Error(`${option} requires a path`);
  return value;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
