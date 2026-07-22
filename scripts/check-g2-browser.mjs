#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { access } from 'node:fs/promises';
import { chromium } from 'playwright-core';

const baseUrl = process.env.PLIEGO_G2_URL ?? 'http://127.0.0.1:4320';
const browser = await chromium.launch({
  executablePath: await findChrome(),
  headless: true,
  args: ['--disable-gpu', '--no-sandbox'],
});

try {
  await runJourney(true, 'browser-js', 'Browser JS');
  await runJourney(false, 'browser-no-js', 'Browser No JS');
  process.stdout.write(`G2 browser acceptance PASS | ${baseUrl} | JavaScript on/off\n`);
} finally {
  await browser.close();
}
async function runJourney(javaScriptEnabled, username, displayName) {
  const context = await browser.newContext({ javaScriptEnabled });
  const page = await context.newPage();
  const errors = [];
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text());
  });
  page.on('pageerror', (error) => errors.push(error.stack || error.message));
  try {
    const login = await page.goto(`${baseUrl}/login`, { waitUntil: 'load' });
    assert.equal(login?.status(), 200);
    assert.equal(await page.locator('script').count(), 0);
    await page.locator('input[name="username"]').fill(username);
    await page.locator('input[name="password"]').fill('preview-only');
    await Promise.all([
      page.waitForURL(`${baseUrl}/dashboard`),
      page.locator('button[type="submit"]').click(),
    ]);
    assert.match(await page.locator('body').innerText(), new RegExp(`Signed in as ${username}`));
    await page.locator('input[name="display_name"]').fill(displayName);
    await Promise.all([
      page.waitForURL(`${baseUrl}/dashboard`),
      page.locator('button[type="submit"]').click(),
    ]);
    assert.equal(await page.locator('h1').innerText(), displayName);
    const catalog = await page.goto(`${baseUrl}/catalog`, { waitUntil: 'load' });
    assert.equal(catalog?.status(), 200);
    assert.match(await page.locator('body').innerText(), new RegExp(displayName));
    assert.deepEqual(errors, []);
  } finally {
    await context.close();
  }
}

async function findChrome() {
  const candidates = [
    process.env.CHROME,
    process.env.CHROME_PATH,
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {}
  }
  throw new Error('Chrome or Chromium was not found; set CHROME_PATH');
}
