// SPDX-License-Identifier: Apache-2.0

import { TextDecoder } from 'node:util';

export const MAX_SOURCE_ARCHIVE_ENTRIES = 10_000;
export const MAX_SOURCE_ARCHIVE_LISTING_BYTES = 4 * 1024 * 1024;
export const MAX_SOURCE_ARCHIVE_ENTRY_BYTES = 4 * 1024;

const SOURCE_ROOT = 'pliegors-source/';
const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f]/u;

export function validateSourceArchiveEntry(entry) {
  if (Buffer.byteLength(entry, 'utf8') > MAX_SOURCE_ARCHIVE_ENTRY_BYTES) {
    throw new Error('source archive entry exceeds byte bound');
  }
  const segments = entry.split('/');
  const interiorSegments = entry.endsWith('/') ? segments.slice(0, -1) : segments;
  if (!entry.startsWith(SOURCE_ROOT)
    || entry.includes('\\')
    || CONTROL_CHARACTERS.test(entry)
    || interiorSegments.some((segment, index) => index > 0 && (segment === '' || segment === '.' || segment === '..'))) {
    throw new Error(`unsafe source archive entry: ${entry}`);
  }
}

export function createSourceArchiveListingParser({
  maxBytes = MAX_SOURCE_ARCHIVE_LISTING_BYTES,
  maxEntries = MAX_SOURCE_ARCHIVE_ENTRIES,
} = {}) {
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 1) throw new Error('invalid source listing byte bound');
  if (!Number.isSafeInteger(maxEntries) || maxEntries < 1) throw new Error('invalid source listing entry bound');

  const decoder = new TextDecoder('utf-8', { fatal: true });
  let bytes = 0;
  let entries = 0;
  let pending = '';
  let finished = false;

  const acceptLine = (rawLine) => {
    const entry = rawLine.endsWith('\r') ? rawLine.slice(0, -1) : rawLine;
    if (entry.length === 0) return;
    validateSourceArchiveEntry(entry);
    entries += 1;
    if (entries > maxEntries) throw new Error('source archive entry count is invalid');
  };

  const drainLines = () => {
    let newline = pending.indexOf('\n');
    while (newline !== -1) {
      acceptLine(pending.slice(0, newline));
      pending = pending.slice(newline + 1);
      newline = pending.indexOf('\n');
    }
    if (Buffer.byteLength(pending, 'utf8') > MAX_SOURCE_ARCHIVE_ENTRY_BYTES + 1) {
      throw new Error('source archive entry exceeds byte bound');
    }
  };

  return Object.freeze({
    write(chunk) {
      if (finished) throw new Error('source archive listing parser is already finished');
      const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      bytes += buffer.length;
      if (bytes > maxBytes) throw new Error('source archive listing exceeds byte bound');
      pending += decoder.decode(buffer, { stream: true });
      drainLines();
    },
    finish() {
      if (finished) throw new Error('source archive listing parser is already finished');
      finished = true;
      pending += decoder.decode();
      drainLines();
      if (pending.length > 0) acceptLine(pending);
      if (entries === 0) throw new Error('source archive entry count is invalid');
      return Object.freeze({ bytes, entries });
    },
  });
}
