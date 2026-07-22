// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  MAX_SOURCE_ARCHIVE_ENTRY_BYTES,
  createSourceArchiveListingParser,
  validateSourceArchiveEntry,
} from './source-archive-listing.mjs';

function writeInChunks(parser, bytes, widths = [1, 7, 31, 257]) {
  let offset = 0;
  let index = 0;
  while (offset < bytes.length) {
    const width = widths[index % widths.length];
    parser.write(bytes.subarray(offset, offset + width));
    offset += width;
    index += 1;
  }
}

test('streaming source listing preserves and validates more than 32 KiB', () => {
  const listing = Array.from(
    { length: 2_000 },
    (_, index) => `pliegors-source/crates/fixture-${String(index).padStart(4, '0')}/src/lib.rs`,
  ).join('\n') + '\n';
  assert.ok(Buffer.byteLength(listing) > 32 * 1024);

  const parser = createSourceArchiveListingParser();
  writeInChunks(parser, Buffer.from(listing));
  assert.deepEqual(parser.finish(), { bytes: Buffer.byteLength(listing), entries: 2_000 });
});

test('unsafe entry remains visible after a listing larger than the old output bound', () => {
  const prefix = Array.from(
    { length: 1_500 },
    (_, index) => `pliegors-source/crates/fixture-${index}/src/lib.rs`,
  ).join('\n');
  const listing = `${prefix}\npliegors-source/crates/../../outside.txt\n`;
  assert.ok(Buffer.byteLength(prefix) > 32 * 1024);

  const parser = createSourceArchiveListingParser();
  assert.throws(() => parser.write(Buffer.from(listing)), /unsafe source archive entry/u);
});

test('source listing rejects excessive entries, bytes, invalid UTF-8, and empty output', () => {
  const tooMany = createSourceArchiveListingParser({ maxEntries: 1 });
  assert.throws(
    () => tooMany.write(Buffer.from('pliegors-source/a\npliegors-source/b\n')),
    /entry count/u,
  );

  const tooLarge = createSourceArchiveListingParser({ maxBytes: 8 });
  assert.throws(() => tooLarge.write(Buffer.from('pliegors-source/a\n')), /byte bound/u);

  const invalidUtf8 = createSourceArchiveListingParser();
  assert.throws(() => invalidUtf8.write(Buffer.from([0xff, 0xfe])), /encoded data|UTF-8|utf-8/iu);

  assert.throws(() => createSourceArchiveListingParser().finish(), /entry count/u);
});

test('source entries must be canonical descendants of the signed root', () => {
  for (const entry of [
    '/pliegors-source/file',
    'pliegors-source/../file',
    'pliegors-source//file',
    'pliegors-source/./file',
    'pliegors-source\\file',
    `pliegors-source/${'a'.repeat(MAX_SOURCE_ARCHIVE_ENTRY_BYTES)}`,
    'pliegors-source/file\u0000name',
  ]) {
    assert.throws(() => validateSourceArchiveEntry(entry), /unsafe|byte bound/u, entry);
  }
  assert.doesNotThrow(() => validateSourceArchiveEntry('pliegors-source/'));
  assert.doesNotThrow(() => validateSourceArchiveEntry('pliegors-source/crates/pliego/src/lib.rs'));
});
