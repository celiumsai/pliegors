#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import path from 'node:path';
import { spawn } from 'node:child_process';
import process from 'node:process';
import { createWranglerConfig, inspectorPortFor } from './wrangler-pboc.mjs';

const root = path.resolve(import.meta.dirname, '..');
const manifestPath = path.resolve(
  root,
  process.argv[2] ?? process.env.PLIEGO_PBOC_MANIFEST ?? '',
);
if (!process.argv[2] && !process.env.PLIEGO_PBOC_MANIFEST) {
  throw new Error('usage: npm run dev -- <path-to-pliego.pboc.json>');
}
const { configPath } = await createWranglerConfig(
  root,
  manifestPath,
  process.env.PLIEGO_CF_NAME ?? 'pliegors-provider-tck',
);

const wrangler = path.join(
  root,
  'node_modules',
  'wrangler',
  'bin',
  'wrangler.js',
);
const port = process.env.PLIEGO_CF_PORT ?? '8788';
const inspectorPort = inspectorPortFor(port, process.env.PLIEGO_CF_INSPECTOR_PORT);
const child = spawn(
  process.execPath,
  [
    wrangler,
    'dev',
    '--config',
    configPath,
    '--local',
    '--port',
    port,
    '--inspector-port',
    inspectorPort,
  ],
  {
    cwd: root,
    env: process.env,
    stdio: 'inherit',
    shell: false,
  },
);

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal));
}
child.on('error', (error) => {
  throw error;
});
child.on('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  process.exit(code ?? 1);
});
