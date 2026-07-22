#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import Ajv2020 from 'ajv/dist/2020.js';

const schema = JSON.parse(await readFile(new URL('../schemas/pliego.pboc.schema.json', import.meta.url)));
const fixture = JSON.parse(await readFile(new URL('../fixtures/pboc/valid-minimal.json', import.meta.url)));
const validate = new Ajv2020({ allErrors: true, strict: true }).compile(schema);
assert.equal(validate(fixture), true, JSON.stringify(validate.errors));
const providerField = structuredClone(fixture);
providerField.accountId = 'provider-private';
assert.equal(validate(providerField), false, 'schema accepted a provider control-plane field');
const credential = structuredClone(fixture);
credential.secretReferences[0] = { id: 'api-key', purpose: 'TOKEN=value', required: true };
assert.equal(validate(credential), true, 'schema should leave semantic secret rejection to Rust validator');
process.stdout.write('PBOC schema contract: PASS | provider fields closed | semantic validation delegated to Rust\n');
