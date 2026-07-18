#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import { lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

const CRC_TABLE = new Uint32Array(256);
for (let index = 0; index < CRC_TABLE.length; index += 1) {
  let value = index;
  for (let bit = 0; bit < 8; bit += 1) value = (value >>> 1) ^ (0xedb88320 & -(value & 1));
  CRC_TABLE[index] = value >>> 0;
}

const arguments_ = parseArguments(process.argv.slice(2));
const root = path.resolve(arguments_.root);
const output = path.resolve(arguments_.output);
const executable = portable(arguments_.executable);
if (output === root || output.startsWith(`${root}${path.sep}`)) {
  throw new Error('ZIP output must be outside its input root');
}
const rootName = path.basename(root);
if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/u.test(rootName)) throw new Error('invalid archive root name');
const files = await collectFiles(root, rootName);
if (!files.some((entry) => entry.name === executable)) throw new Error('declared executable is absent from the archive');

const localParts = [];
const centralParts = [];
let offset = 0;
for (const entry of files) {
  const name = Buffer.from(entry.name, 'utf8');
  const bytes = await readFile(entry.file);
  if (bytes.length > 0xffff_ffff) throw new Error('ZIP64 inputs are not supported');
  const checksum = crc32(bytes);
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4);
  local.writeUInt16LE(0x0800, 6);
  local.writeUInt16LE(0, 8);
  local.writeUInt16LE(0, 10);
  local.writeUInt16LE(0x0021, 12);
  local.writeUInt32LE(checksum, 14);
  local.writeUInt32LE(bytes.length, 18);
  local.writeUInt32LE(bytes.length, 22);
  local.writeUInt16LE(name.length, 26);
  local.writeUInt16LE(0, 28);
  localParts.push(local, name, bytes);

  const central = Buffer.alloc(46);
  central.writeUInt32LE(0x02014b50, 0);
  central.writeUInt16LE(0x0314, 4);
  central.writeUInt16LE(20, 6);
  central.writeUInt16LE(0x0800, 8);
  central.writeUInt16LE(0, 10);
  central.writeUInt16LE(0, 12);
  central.writeUInt16LE(0x0021, 14);
  central.writeUInt32LE(checksum, 16);
  central.writeUInt32LE(bytes.length, 20);
  central.writeUInt32LE(bytes.length, 24);
  central.writeUInt16LE(name.length, 28);
  central.writeUInt16LE(0, 30);
  central.writeUInt16LE(0, 32);
  central.writeUInt16LE(0, 34);
  central.writeUInt16LE(0, 36);
  const mode = entry.name === executable ? 0o100755 : 0o100644;
  central.writeUInt32LE(mode * 0x10000, 38);
  central.writeUInt32LE(offset, 42);
  centralParts.push(central, name);
  offset += local.length + name.length + bytes.length;
}

const centralOffset = offset;
const centralDirectory = Buffer.concat(centralParts);
const end = Buffer.alloc(22);
end.writeUInt32LE(0x06054b50, 0);
end.writeUInt16LE(0, 4);
end.writeUInt16LE(0, 6);
end.writeUInt16LE(files.length, 8);
end.writeUInt16LE(files.length, 10);
end.writeUInt32LE(centralDirectory.length, 12);
end.writeUInt32LE(centralOffset, 16);
end.writeUInt16LE(0, 20);
await mkdir(path.dirname(output), { recursive: true });
await writeFile(output, Buffer.concat([...localParts, centralDirectory, end]), { flag: 'wx' });
console.log(`Deterministic ZIP: ${files.length} files -> ${output}`);

function parseArguments(argv) {
  const allowed = new Set(['--root', '--output', '--executable']);
  const result = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!allowed.has(option) || !value || value.startsWith('--') || Object.hasOwn(result, option)) {
      throw new Error(`invalid or incomplete option: ${option ?? '<missing>'}`);
    }
    result[option] = value;
  }
  for (const option of allowed) if (!result[option]) throw new Error(`${option} is required`);
  return { root: result['--root'], output: result['--output'], executable: result['--executable'] };
}

async function collectFiles(directory, prefix) {
  const result = [];
  const queue = [[directory, prefix]];
  let observed = 0;
  while (queue.length > 0) {
    const [current, portablePrefix] = queue.shift();
    const entries = await readdir(current, { withFileTypes: true });
    entries.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      observed += 1;
      if (observed > 100) throw new Error('archive input exceeds 100 entries');
      if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/u.test(entry.name)) throw new Error(`invalid archive entry: ${entry.name}`);
      const file = path.join(current, entry.name);
      const stat = await lstat(file);
      if (stat.isSymbolicLink()) throw new Error(`archive input contains a link: ${entry.name}`);
      const name = `${portablePrefix}/${entry.name}`;
      if (entry.isDirectory()) queue.push([file, name]);
      else if (entry.isFile()) result.push({ file, name });
      else throw new Error(`unsupported archive entry: ${entry.name}`);
    }
  }
  if (result.length === 0 || result.length > 0xffff) throw new Error('invalid ZIP entry count');
  return result.sort((left, right) => left.name.localeCompare(right.name));
}

function portable(value) {
  if (value.includes('\\') || value.startsWith('/') || value.split('/').includes('..')) {
    throw new Error('executable path must be portable and relative');
  }
  return value;
}

function crc32(bytes) {
  let crc = 0xffff_ffff;
  for (const byte of bytes) crc = CRC_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffff_ffff) >>> 0;
}
