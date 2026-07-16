import assert from "node:assert/strict";
import test from "node:test";
import worker from "./src/preview.mjs";

const env = {
  ASSETS: {
    async fetch() {
      return new Response("site", {
        status: 203,
        headers: { "Content-Type": "text/html" },
      });
    },
  },
};

test("protected preview denies crawlers through robots and response headers", async () => {
  const robots = await worker.fetch(new Request("https://pliegors.dev/robots.txt"), env);
  assert.equal(robots.status, 200);
  assert.equal(await robots.text(), "User-agent: *\nDisallow: /\n");
  assert.equal(robots.headers.get("x-robots-tag"), "noindex, nofollow, noarchive");
  assert.equal(robots.headers.get("cache-control"), "no-store");

  const page = await worker.fetch(new Request("https://pliegors.dev/docs/"), env);
  assert.equal(page.status, 203);
  assert.equal(await page.text(), "site");
  assert.equal(page.headers.get("content-type"), "text/html");
  assert.equal(page.headers.get("x-robots-tag"), "noindex, nofollow, noarchive");

  const security = await worker.fetch(
    new Request("https://pliegors.dev/.well-known/security.txt"),
    env,
  );
  assert.equal(security.status, 203);
  assert.equal(security.headers.get("content-type"), "text/plain; charset=utf-8");
  assert.equal(security.headers.get("x-robots-tag"), "noindex, nofollow, noarchive");
});
