#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const schema = JSON.parse(await readFile(path.join(root, 'schemas', 'pliego.telemetry-report.schema.json'), 'utf8'));
const telemetry = await readFile(path.join(root, 'crates', 'pliego-cli', 'src', 'telemetry.rs'), 'utf8');
const manifest = await readFile(path.join(root, 'crates', 'pliego-cli', 'Cargo.toml'), 'utf8');
const runner = await readFile(path.join(root, 'scripts', 'run-golden-path.mjs'), 'utf8');
const documentation = await readFile(path.join(root, 'docs', '41-voluntary-telemetry.md'), 'utf8');

const validate = new Ajv2020({ allErrors: true, strict: true }).compile(schema);
const sample = {
  contract: 'dev.pliegors.telemetry-report/v1',
  generatedAtDay: 20_652,
  consent: { enabled: true, policyVersion: '1.0.0', enabledAtDay: 20_652 },
  fields: ['sequence', 'event', 'daySinceUnixEpoch', 'cliVersion', 'platform', 'architecture'],
  events: [{
    sequence: 0,
    event: 'install',
    daySinceUnixEpoch: 20_652,
    cliVersion: '0.0.2',
    platform: 'linux',
    architecture: 'x86_64',
  }],
};
assert.equal(validate(sample), true, JSON.stringify(validate.errors));
for (const event of ['Install', 'New', 'Check', 'Dev', 'Build']) {
  assert.match(telemetry, new RegExp(`\\b${event},`, 'u'), `telemetry allowlist lacks ${event}`);
}
for (const token of [
  'MAX_EVENTS: usize = 64',
  'network_submission: "none"',
  'create_new(true)',
  '--delete-local',
  'No data was transmitted.',
]) assert.ok(telemetry.includes(token), `telemetry implementation lacks ${token}`);
for (const forbidden of ['reqwest', 'ureq', 'hyper', 'PLIEGO_TELEMETRY', 'https://', 'http://']) {
  assert.ok(!telemetry.includes(forbidden), `telemetry implementation contains forbidden network/enable surface: ${forbidden}`);
}
for (const networkDependency of ['reqwest', 'ureq', 'hyper', 'tokio']) {
  assert.doesNotMatch(manifest, new RegExp(`^${networkDependency}\\s*=`, 'mu'), `CLI adds network dependency ${networkDependency}`);
}
for (const step of ['telemetry-default-before', 'telemetry-default-after']) {
  assert.ok(runner.includes(`step('${step}'`), `golden path lacks ${step}`);
}
for (const command of ['telemetry enable', 'telemetry preview', 'telemetry export', 'telemetry disable --delete-local']) {
  assert.ok(documentation.includes(command), `telemetry docs lack ${command}`);
}

const expanded = structuredClone(sample);
expanded.events[0].project = 'secret-project';
assert.equal(validate(expanded), false, 'schema accepts a project identifier');
const unknownEvent = structuredClone(sample);
unknownEvent.events[0].event = 'route';
assert.equal(validate(unknownEvent), false, 'schema accepts an event outside the funnel');
const oversized = structuredClone(sample);
oversized.events = Array.from({ length: 65 }, (_, sequence) => ({ ...sample.events[0], sequence }));
assert.equal(validate(oversized), false, 'schema accepts more than 64 events');

console.log('Voluntary telemetry contract PASS: disabled by default, local-only, bounded, and user-deletable');
