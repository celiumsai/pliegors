#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { existsSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import process from 'node:process';

const root = path.resolve(import.meta.dirname, '..');
const installRoot = path.join(root, 'target', 'worker-build-tool');
const executable = path.join(
  installRoot,
  'bin',
  process.platform === 'win32' ? 'worker-build.exe' : 'worker-build',
);
const output = path.join(root, 'build', 'worker', 'shim.mjs');

if (process.env.PLIEGO_SKIP_WORKER_BUILD === '1' && existsSync(output)) {
  process.exit(0);
}

if (!existsSync(executable)) {
  run('cargo', [
    '+stable',
    'install',
    'worker-build',
    '--version',
    '0.7.5',
    '--locked',
    '--root',
    installRoot,
  ]);
}
run(executable, ['--release']);

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    env: process.env,
    stdio: 'inherit',
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
