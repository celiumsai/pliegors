// SPDX-License-Identifier: Apache-2.0

import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const sourceRoots = [
  'crates/pliego-dom',
  'crates/pliego-adapters',
  'examples/spike',
  'examples/pliegors-site-client',
];
const forbidden = [
  { name: 'Closure::forget', pattern: /\bClosure::forget\s*\(/u },
  { name: '.forget()', pattern: /\.forget\s*\(\s*\)/u },
  { name: 'mem::forget', pattern: /\b(?:std::)?mem::forget\s*\(/u },
];

async function rustFiles(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const target = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await rustFiles(target));
    else if (entry.isFile() && entry.name.endsWith('.rs')) files.push(target);
  }
  return files;
}

const files = (await Promise.all(sourceRoots.map((directory) => rustFiles(path.join(root, directory)))))
  .flat()
  .sort();
const failures = [];
for (const file of files) {
  const source = await readFile(file, 'utf8');
  const lines = source.split(/\r?\n/u);
  for (const { name, pattern } of forbidden) {
    lines.forEach((line, index) => {
      if (pattern.test(line)) {
        failures.push(`${path.relative(root, file)}:${index + 1}: forbidden ${name}`);
      }
    });
  }
}

if (failures.length > 0) {
  throw new Error(`unowned WASM lifetime escapes found:\n${failures.join('\n')}`);
}
console.log(`WASM lifetime contract PASS: ${files.length} Rust source files contain no forget escape`);
