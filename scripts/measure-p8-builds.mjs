// SPDX-License-Identifier: Apache-2.0

import { spawn, spawnSync } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const samples = parseInteger(process.argv[2] ?? '10', 'samples', 1, 100);
const requestedOutput = process.argv[3] ?? 'target/benchmarks/p8-build.json';
const output = path.resolve(root, requestedOutput);
const revision = commandText('git', ['rev-parse', '--verify', 'HEAD'], root);
const dirty = commandText('git', ['status', '--porcelain', '--untracked-files=normal'], root) !== '';

if (!/^[0-9a-f]{40}$/u.test(revision)) {
  throw new Error('benchmark source revision is not a full Git commit SHA');
}
if (dirty && process.env.PLIEGORS_ALLOW_DIRTY_BENCH !== '1') {
  throw new Error('refusing benchmark evidence from a dirty tree; commit first or use PLIEGORS_ALLOW_DIRTY_BENCH=1 for a non-evidence smoke run');
}

const temporary = await mkdtemp(path.join(os.tmpdir(), 'pliegors-p8-build-'));
const cliTarget = path.join(temporary, 'cli-target');
const cli = path.join(cliTarget, 'release', process.platform === 'win32' ? 'pliego.exe' : 'pliego');
const observations = [];

try {
  await run('cargo', ['build', '--manifest-path', path.join(root, 'Cargo.toml'), '-p', 'pliego-cli', '--release', '--locked'], root, {
    CARGO_TARGET_DIR: cliTarget,
  });

  for (let sample = 1; sample <= samples; sample += 1) {
    const workspace = path.join(temporary, `sample-${sample}`, 'workspace');
    const project = path.join(workspace, 'examples', 'content-collections-pliegors');
    await createFixture(workspace, project);
    await run('cargo', ['generate-lockfile', '--manifest-path', path.join(project, 'Cargo.toml')], project);

    const observation = { sample };
    observation.cleanColdBuildMs = await timedBuild(cli, project);
    observation.noChangeWarmMs = await timedBuild(cli, project);

    await append(path.join(workspace, 'fixtures', 'content', 'reference', 'journal', 'content-as-data.md'), `\n\nBenchmark content revision ${sample}.\n`);
    observation.contentOnlyMs = await timedBuild(cli, project);

    await append(path.join(project, 'assets', 'site.css'), `\n/* benchmark-css-${sample} */\n`);
    observation.cssOnlyMs = await timedBuild(cli, project);

    await append(path.join(project, 'src', 'components.rs'), `\nconst _BENCHMARK_RUST_VIEW_${sample}: &str = \"sample-${sample}\";\n`);
    observation.rustViewMs = await timedBuild(cli, project);
    observations.push(observation);
    process.stdout.write(`sample ${String(sample).padStart(2, '0')}/${String(samples).padStart(2, '0')}: ${formatObservation(observation)}\n`);
  }

  const metrics = ['cleanColdBuildMs', 'noChangeWarmMs', 'contentOnlyMs', 'cssOnlyMs', 'rustViewMs'];
  const summary = Object.fromEntries(metrics.map((metric) => [metric, summarize(observations.map((entry) => entry[metric]))]));
  const report = {
    contract: 'dev.pliegors.p8-build-benchmark/v1',
    revision,
    sourceTreeDirty: dirty,
    createdAt: new Date().toISOString(),
    sampleCount: samples,
    percentileMethod: 'nearest-rank',
    environment: environmentReport(),
    fixture: {
      project: 'examples/content-collections-pliegors',
      topology: 'standalone copy with local path dependencies to the measured revision',
      dependencyResolution: 'Cargo.lock generated before every timed sample',
    },
    cachePolicy: {
      releaseCli: 'built once outside timed regions',
      cleanColdBuild: 'fresh project and empty Cargo target directory per sample',
      subsequentBuilds: 'same target and project, in declared mutation order',
      cargoRegistryAndGit: 'host caches retained and reported as an explicit limitation',
    },
    measuredCommand: 'pliego build',
    mutationOrder: ['none/fresh target', 'none/warm target', 'Markdown content', 'included CSS', 'Rust view source'],
    rawObservations: observations,
    summary,
    limitations: [
      'Results apply only to the recorded revision, hardware, operating system, toolchain, and cache policy.',
      'The clean build retains host Cargo registry and Git caches; it clears only the project build target.',
      'No competitor framework is measured or compared by this report.',
    ],
  };
  await mkdir(path.dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  process.stdout.write(`build benchmark: ${output}\n`);
} finally {
  await rm(temporary, { recursive: true, force: true });
}

function parseInteger(value, label, minimum, maximum) {
  if (!/^[0-9]+$/u.test(value)) throw new Error(`${label} must be an integer`);
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < minimum || number > maximum) {
    throw new Error(`${label} must be between ${minimum} and ${maximum}`);
  }
  return number;
}

function commandText(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed: ${(result.stderr ?? '').trim()}`);
  return (result.stdout ?? '').trim();
}

async function run(command, args, cwd, extraEnvironment = {}) {
  const environment = { ...process.env, ...extraEnvironment };
  if (!Object.hasOwn(extraEnvironment, 'CARGO_TARGET_DIR')) delete environment.CARGO_TARGET_DIR;
  await new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, env: environment, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => { stdout = bounded(stdout, chunk); });
    child.stderr.on('data', (chunk) => { stderr = bounded(stderr, chunk); });
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} ${args.join(' ')} failed (${signal ?? code})\n${stdout}\n${stderr}`));
    });
  });
}

function bounded(current, chunk) {
  return `${current}${chunk.toString('utf8')}`.slice(-16_384);
}

async function timedBuild(cliPath, project) {
  const start = process.hrtime.bigint();
  await run(cliPath, ['build'], project);
  return roundMilliseconds(Number(process.hrtime.bigint() - start) / 1_000_000);
}

async function createFixture(workspace, project) {
  await mkdir(project, { recursive: true });
  await Promise.all([
    cp(path.join(root, 'examples', 'content-collections-pliegors', 'src'), path.join(project, 'src'), { recursive: true }),
    cp(path.join(root, 'examples', 'content-collections-pliegors', 'assets'), path.join(project, 'assets'), { recursive: true }),
    cp(path.join(root, 'fixtures', 'content', 'reference'), path.join(workspace, 'fixtures', 'content', 'reference'), { recursive: true }),
    mkdir(path.join(workspace, 'brand'), { recursive: true }),
    mkdir(path.join(workspace, 'crates', 'pliego-starters', 'templates', 'editorial', 'assets', 'images'), { recursive: true }),
  ]);
  await Promise.all([
    cp(path.join(root, 'brand', 'pliegors-symbol.svg'), path.join(workspace, 'brand', 'pliegors-symbol.svg')),
    cp(path.join(root, 'brand', 'pliegors-app-icon.svg'), path.join(workspace, 'brand', 'pliegors-app-icon.svg')),
    cp(path.join(root, 'crates', 'pliego-starters', 'templates', 'editorial', 'assets', 'images', 'hero.jpg'), path.join(workspace, 'crates', 'pliego-starters', 'templates', 'editorial', 'assets', 'images', 'hero.jpg')),
    cp(path.join(root, 'examples', 'content-collections-pliegors', 'pliego.toml'), path.join(project, 'pliego.toml')),
  ]);
  const dependency = (name) => path.join(root, 'crates', name).replaceAll('\\', '/');
  const manifest = `# SPDX-License-Identifier: Apache-2.0\n[package]\nname = "content-collections-pliegors"\nversion = "0.0.0"\nedition = "2024"\nrust-version = "1.85"\npublish = false\n\n[workspace]\n\n[[bin]]\nname = "content-collections-pliegors"\npath = "src/main.rs"\ntest = false\nbench = false\n\n[dependencies]\npliego-content = { path = "${dependency('pliego-content')}" }\npliego-dom = { path = "${dependency('pliego-dom')}" }\npliego-ssg = { path = "${dependency('pliego-ssg')}" }\nserde = { version = "1", features = ["derive"] }\n`;
  await writeFile(path.join(project, 'Cargo.toml'), manifest, 'utf8');
}

async function append(file, value) {
  const current = await readFile(file, 'utf8');
  await writeFile(file, `${current}${value}`, 'utf8');
}

function summarize(values) {
  const ordered = [...values].sort((left, right) => left - right);
  return {
    p50Ms: nearestRank(ordered, 0.50),
    p95Ms: nearestRank(ordered, 0.95),
  };
}

function nearestRank(ordered, percentile) {
  return ordered[Math.ceil(percentile * ordered.length) - 1];
}

function roundMilliseconds(value) {
  return Math.round(value * 1_000) / 1_000;
}

function environmentReport() {
  const cpu = os.cpus()[0];
  return {
    platform: process.platform,
    architecture: process.arch,
    osRelease: os.release(),
    cpuModel: cpu?.model?.trim() ?? 'unknown',
    logicalCpuCount: os.cpus().length,
    totalMemoryBytes: os.totalmem(),
    storageClass: process.env.PLIEGORS_BENCH_STORAGE_CLASS ?? 'not declared',
    node: process.version,
    rustc: commandText('rustc', ['--version', '--verbose'], root),
    cargo: commandText('cargo', ['--version', '--verbose'], root),
    powerAndThermalState: 'not controlled or measured',
  };
}

function formatObservation(observation) {
  return `cold ${observation.cleanColdBuildMs} ms, warm ${observation.noChangeWarmMs} ms, content ${observation.contentOnlyMs} ms, CSS ${observation.cssOnlyMs} ms, Rust ${observation.rustViewMs} ms`;
}
