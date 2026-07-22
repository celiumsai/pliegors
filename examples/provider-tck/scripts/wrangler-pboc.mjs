// SPDX-License-Identifier: Apache-2.0

import { createHash } from 'node:crypto';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

export async function createWranglerConfig(root, manifestPath, name) {
  const absoluteManifest = path.resolve(manifestPath);
  const source = (await readFile(absoluteManifest, 'utf8')).trim();
  const manifest = JSON.parse(source);
  const bundle = path.dirname(absoluteManifest);
  const target = manifest.targets.find(({ id }) => id === 'cloudflare-workers');
  if (!target) throw new Error('PBOC does not contain cloudflare-workers');
  const mainArtifact = target.artifactPaths.find((candidate) => candidate.endsWith('/worker/pboc.mjs'));
  if (!mainArtifact) throw new Error('Cloudflare target lacks worker/pboc.mjs');

  const main = inside(bundle, mainArtifact);
  const assets = inside(bundle, 'public');
  await Promise.all([access(main), access(assets)]);
  for (const artifact of target.artifactPaths) await access(inside(bundle, artifact));

  const identity = createHash('sha256').update(source).digest('hex');
  const directory = path.join(root, 'target', `wrangler-pboc-${identity.slice(0, 16)}`);
  await mkdir(directory, { recursive: true });
  const configPath = path.join(directory, 'wrangler.json');
  const relative = (value) => path.relative(directory, value).replaceAll(path.sep, '/');
  await writeFile(
    configPath,
    `${JSON.stringify({
      name,
      main: relative(main),
      compatibility_date: '2026-07-20',
      workers_dev: true,
      assets: {
        directory: relative(assets),
        binding: 'ASSETS',
        run_worker_first: ['/', '/api/*', '/health', '/stream'],
      },
      rules: [{ type: 'Text', globs: ['**/*.pboc.json'], fallthrough: true }],
      observability: { enabled: true, head_sampling_rate: 1 },
    }, null, 2)}\n`,
  );
  return { configPath, identity, source };
}

function inside(root, portablePath) {
  const resolved = path.resolve(root, ...portablePath.split('/'));
  const relative = path.relative(root, resolved);
  if (!relative || relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new Error(`PBOC path escapes bundle: ${portablePath}`);
  }
  return resolved;
}
