// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import test from 'node:test';
import { inspectorPortFor } from '../examples/provider-tck/scripts/wrangler-pboc.mjs';

test('parallel provider Workers receive distinct bounded inspector ports', () => {
  assert.equal(inspectorPortFor('8788'), '18788');
  assert.equal(inspectorPortFor('8789'), '18789');
  assert.equal(inspectorPortFor('8788', '19001'), '19001');
  assert.throws(() => inspectorPortFor('8788', '0'), /outside/u);
  assert.throws(() => inspectorPortFor('8788', 'debug'), /numeric/u);
});
