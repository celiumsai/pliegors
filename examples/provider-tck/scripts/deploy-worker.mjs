#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import path from 'node:path';
import { spawnSync } from 'node:child_process';
import process from 'node:process';
import { createWranglerConfig } from './wrangler-pboc.mjs';

const root = path.resolve(import.meta.dirname, '..');
const manifestPath = process.argv[2];
if (!manifestPath) {
  throw new Error('usage: node scripts/deploy-worker.mjs <pliego.pboc.json> [--dry-run]');
}
const { configPath } = await createWranglerConfig(
  root,
  path.resolve(root, manifestPath),
  process.env.PLIEGO_CF_NAME ?? 'pliegors-provider-tck',
);
const wrangler = path.join(root, 'node_modules', 'wrangler', 'bin', 'wrangler.js');
const arguments_ = [
  wrangler,
  'deploy',
  '--config',
  configPath,
];
if (process.argv.includes('--dry-run')) arguments_.push('--dry-run');
const result = spawnSync(process.execPath, arguments_, {
  cwd: root,
  env: process.env,
  stdio: 'inherit',
  shell: false,
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
