#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { existsSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import process from 'node:process';

const root = path.resolve(import.meta.dirname, '..');
const output = path.join(root, 'build', 'worker', 'shim.mjs');
if (!existsSync(output)) {
  throw new Error('Cloudflare Worker output is missing; run npm run build first');
}
const executable = path.join(
  root,
  'node_modules',
  '.bin',
  process.platform === 'win32' ? 'wrangler.cmd' : 'wrangler',
);
const result = spawnSync(
  executable,
  ['deploy', '--dry-run', '--outdir', 'target/wrangler-dry-run'],
  {
    cwd: root,
    env: { ...process.env, PLIEGO_SKIP_WORKER_BUILD: '1' },
    stdio: 'inherit',
    shell: process.platform === 'win32',
  },
);
if (result.error) throw result.error;
process.exit(result.status ?? 1);
