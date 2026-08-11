#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-only

import assert from "node:assert/strict";
import { access } from "node:fs/promises";
import { chromium } from "playwright-core";

const baseUrl = process.env.PLIEGO_HYPHAE_CONSOLE_URL;
assert(baseUrl, "PLIEGO_HYPHAE_CONSOLE_URL is required");
const expectedOrigin = new URL(baseUrl).origin;
const browser = await chromium.launch({
  executablePath: await findChrome(),
  headless: true,
  args: ["--disable-gpu", "--no-sandbox"],
});

try {
  const alice = await journey("alice", true, 2);
  const bob = await journey("bob", false, 1);
  assert.match(alice, /Counter 2/u);
  assert.doesNotMatch(alice, /tenant-b/u);
  assert.match(bob, /Counter 1/u);
  assert.doesNotMatch(bob, /tenant-a/u);
  process.stdout.write(`Hyphae Console browser isolation PASS: ${expectedOrigin}\n`);
} finally {
  await browser.close();
}

async function journey(username, javaScriptEnabled, increments) {
  const context = await browser.newContext({ javaScriptEnabled });
  const page = await context.newPage();
  const errors = [];
  const requests = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.stack || error.message));
  page.on("request", (request) => requests.push(request.url()));
  try {
    const login = await page.goto(`${baseUrl}/login`, { waitUntil: "load" });
    assert.equal(login?.status(), 200);
    assert.equal(await page.locator("script").count(), 0);
    await page.locator('input[name="username"]').fill(username);
    await page.locator('input[name="password"]').fill("preview-only");
    const loginBody = new URLSearchParams({
      username,
      password: "preview-only",
      _csrf: await page.locator('input[name="_csrf"]').inputValue(),
    }).toString();
    const loginResponse = await context.request.post(`${baseUrl}/login`, {
      headers: {
        origin: expectedOrigin,
        "content-type": "application/x-www-form-urlencoded",
      },
      data: loginBody,
      maxRedirects: 0,
    });
    assert.equal(
      loginResponse.status(),
      303,
      `${await loginResponse.text()}\ncookies=${JSON.stringify(await context.cookies())}`,
    );
    await page.goto(`${baseUrl}/console`, { waitUntil: "load" });
    for (let index = 0; index < increments; index += 1) {
      const incrementBody = new URLSearchParams({
        expected_revision: await page.locator('input[name="expected_revision"]').inputValue(),
        _csrf: await page.locator('input[name="_csrf"]').inputValue(),
      }).toString();
      const mutation = await context.request.post(`${baseUrl}/console/increment`, {
        headers: {
          origin: expectedOrigin,
          "content-type": "application/x-www-form-urlencoded",
        },
        data: incrementBody,
        maxRedirects: 0,
      });
      assert.equal(mutation.status(), 303, await mutation.text());
      await page.reload({ waitUntil: "load" });
    }
    const html = await page.content();
    const activity = await page.goto(`${baseUrl}/console/activity`, { waitUntil: "load" });
    assert.equal(activity?.status(), 200);
    assert.equal(await page.locator("article").count(), increments);
    const raw = await context.request.get(`${baseUrl}/v2/capabilities`);
    assert.equal(raw.status(), 404);
    for (const request of requests) {
      assert.equal(new URL(request).origin, expectedOrigin);
      assert.doesNotMatch(request, /\/v2\/(?:execute|capabilities).*8788/u);
    }
    for (const forbidden of [
      "application/vnd.hyphae.product-v1",
      "application/vnd.hyphae.error-v1",
      "84161cf067141b60f4847b965ef77c5b749749c0",
    ]) {
      assert.doesNotMatch(html, new RegExp(forbidden, "u"));
    }
    assert.deepEqual(errors, []);
    return html;
  } finally {
    await context.close();
  }
}

async function findChrome() {
  const candidates = [
    process.env.CHROME,
    process.env.CHROME_PATH,
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
    "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {}
  }
  throw new Error("Chrome or Chromium was not found; set CHROME_PATH");
}
