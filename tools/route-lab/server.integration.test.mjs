import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer, request as httpRequest } from "node:http";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { gunzipSync, gzipSync } from "node:zlib";

const routeLabRoot = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(routeLabRoot, "..", "..");
const serverPath = path.join(routeLabRoot, "server.mjs");
const targetIds = [
  "reference-alpha",
  "reference-beta",
  "reference-gamma",
];

test("Route Lab serves isolated target origins with verifiable instrumentation", { timeout: 30_000 }, async (t) => {
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "pliego-route-lab-test-"));
  const temporaryRawRoot = path.join(temporaryRoot, "raw");
  const fixture = await writeIntegrationFixture(temporaryRoot);
  const upstreams = await Promise.all(targetIds.map((targetId) => startUpstream(targetId)));
  const dashboardPort = await findConsecutivePorts(4);
  const routeLab = startRouteLab({
    dashboardPort,
    outputRoot: path.join(temporaryRoot, "inbox"),
    rawRoot: temporaryRawRoot,
    upstreams,
    fixture,
  });

  t.after(async () => {
    await stopChild(routeLab);
    await Promise.all(upstreams.map(({ server }) => closeServer(server)));
    await rm(temporaryRoot, { recursive: true, force: true });
  });

  await waitForRouteLab(routeLab, dashboardPort, 4);

  await t.test("opens the dashboard plus one listener per target", async () => {
    const expectedListeners = ["dashboard", ...targetIds];
    const health = await Promise.all(
      expectedListeners.map((_, index) => request({ port: dashboardPort + index, pathname: "/_pliego/health" })),
    );

    for (const [index, result] of health.entries()) {
      assert.equal(result.statusCode, 200);
      assert.equal(result.json().healthy, true);
      assert.equal(result.json().listener, expectedListeners[index]);
    }
  });

  const configResponse = await request({
    port: dashboardPort,
    pathname: "/_pliego/api/config",
  });
  assert.equal(configResponse.statusCode, 200);
  const config = configResponse.json();
  const fingerprint = config.fingerprints.find(
    (candidate) => candidate.deviceId === "integration-device",
  );
  assert.ok(fingerprint, "the integration fixture requires its synthetic fingerprint");
  const fingerprintValue = JSON.parse(
    await readFile(path.join(fixture.fingerprints, fingerprint.fileName), "utf8"),
  );

  const sessions = new Map();
  for (const targetId of targetIds) {
    const response = await request({
      port: dashboardPort,
      pathname: "/_pliego/api/sessions",
      method: "POST",
      json: {
        deviceId: "integration-device",
        hardwareFingerprint: fingerprint.fileName,
        targetId,
        route: "/",
        tier: "universal",
        cacheMode: "cold",
        motionMode: "default",
        orientation: "landscape",
        networkProfile: "desktop-broadband",
        powerState: "external-power",
        thermalState: "nominal",
      },
    });
    assert.equal(response.statusCode, 201, response.body.toString("utf8"));
    sessions.set(targetId, {
      ...response.json(),
      targetPort: dashboardPort + targetIds.indexOf(targetId) + 1,
    });
  }

  await t.test("binds launch URLs and session cookies to the selected target origin", async () => {
    for (const [targetId, record] of sessions) {
      const launch = new URL(record.launchUrl);
      assert.equal(launch.hostname, "127.0.0.1");
      assert.equal(Number(launch.port), record.targetPort);

      const launchResponse = await request({
        port: record.targetPort,
        pathname: `${launch.pathname}${launch.search}`,
      });
      assert.equal(launchResponse.statusCode, 302);
      assert.equal(launchResponse.headers["clear-site-data"], '"cache"');
      assert.equal(
        launchResponse.headers.location,
        `/?__pliego_run=${record.session.id}`,
      );
      const sessionCookie = onlyCookie(launchResponse);
      assert.match(
        sessionCookie,
        new RegExp(`^pliego_route_session_${cookieKey(targetId)}=${record.session.id}$`),
      );
      record.sessionCookie = sessionCookie;
      record.bootstrapPath = launchResponse.headers.location;
    }

    const site = sessions.get("reference-alpha");
    const wrongOrigin = sessions.get("reference-beta");
    const wrongLaunch = await request({
      port: wrongOrigin.targetPort,
      pathname: `/_pliego/launch?session=${site.session.id}`,
    });
    assert.equal(wrongLaunch.statusCode, 404);

    const wrongCookie = await request({
      port: wrongOrigin.targetPort,
      pathname: "/",
      headers: { cookie: site.sessionCookie },
    });
    assert.equal(wrongCookie.statusCode, 302);
    assert.equal(wrongCookie.headers.location, `http://127.0.0.1:${dashboardPort}/_pliego/`);
  });

  await t.test("preserves gzip while injecting the nonce-bearing probe before authored scripts", async () => {
    for (const [targetId, record] of sessions) {
      const response = await request({
        port: record.targetPort,
        pathname: record.bootstrapPath,
        headers: {
          cookie: record.sessionCookie,
          "accept-encoding": "gzip",
          "if-none-match": '"stale-browser-copy"',
        },
      });
      assert.equal(response.statusCode, 200);
      assert.equal(response.headers["content-encoding"], "gzip");
      assert.equal(response.headers["x-pliego-route-lab"], "0.2.2");
      assert.equal(response.headers["cache-control"], "no-store");
      assert.equal(response.headers.etag, undefined);

      const html = gunzipSync(response.body).toString("utf8");
      assert.ok(
        html.indexOf("__PLIEGO_REQUESTED_TIER__") <
          html.indexOf("/_pliego/probe-contract.js"),
      );
      assert.ok(
        html.indexOf("/_pliego/probe-contract.js") < html.indexOf("/_pliego/probe.js"),
      );
      assert.ok(html.indexOf("/_pliego/probe.js") < html.indexOf("/authored.js"));
      assert.match(html, /data-pliego-route-lab/);
      assert.match(html, /__PLIEGO_REQUESTED_TIER__="universal"/);
      assert.match(html, /__PLIEGO_REQUESTED_MOTION__="default"/);

      const nonce = response.headers["content-security-policy"].match(/'nonce-([^']+)'/)?.[1];
      assert.ok(nonce, "the rewritten CSP must authorize the injected probe nonce");
      assert.ok(html.includes(`nonce="${nonce}"`));
      assert.equal(html.split(`nonce="${nonce}"`).length - 1, 3);

      const targetCookie = cookieWithPrefix(
        response,
        `pliego_target_${cookieKey(targetId)}_shared=`,
      );
      assert.ok(targetCookie);
      assert.doesNotMatch(targetCookie, /Domain=/i);
      record.targetCookie = targetCookie;
      record.instrumentedHtml = html;
    }

    for (const upstream of upstreams) {
      const bootstrapRequest = upstream.requests.find((entry) => entry.url === "/");
      assert.ok(bootstrapRequest, `${upstream.targetId} did not receive the bootstrap request`);
      assert.equal(bootstrapRequest.headers["if-none-match"], undefined);
      assert.equal(bootstrapRequest.headers["cache-control"], "no-cache");
      assert.equal(bootstrapRequest.url.includes("__pliego_run"), false);
    }
  });

  await t.test("namespaces identical upstream cookie names without cross-target forwarding", async () => {
    const allTargetCookies = [...sessions.values()].map((record) => record.targetCookie);

    for (const [targetId, record] of sessions) {
      const response = await request({
        port: record.targetPort,
        pathname: "/echo-cookie",
        headers: {
          cookie: [record.sessionCookie, ...allTargetCookies].join("; "),
        },
      });
      assert.equal(response.statusCode, 200);
      assert.equal(response.json().cookie, `shared=${targetId}`);
    }
  });

  await t.test("seals sourceRevision from the exact pre-injection HTML bytes", async () => {
    for (const [targetId, record] of sessions) {
      assert.match(
        record.session.sourceRevision,
        /^pending;manifest-sha256:[a-f0-9]{64}$/,
      );
      const sessionResponse = await request({
        port: record.targetPort,
        pathname: "/_pliego/api/session",
        headers: { cookie: record.sessionCookie },
      });
      assert.equal(sessionResponse.statusCode, 200);

      const sourceRevision = sessionResponse.json().sourceRevision;
      const upstream = upstreams.find((candidate) => candidate.targetId === targetId);
      const expectedHtmlSha = createHash("sha256").update(upstream.html).digest("hex");
      const manifestSha = record.session.sourceRevision.split("manifest-sha256:")[1];
      assert.equal(
        sourceRevision,
        `html-sha256:${expectedHtmlSha};manifest-sha256:${manifestSha}`,
      );
    }
  });

  await t.test("accepts a 1.1 segment and persists its reconciled first-route ledger", async () => {
    const record = sessions.get("reference-alpha");
    const segment = measurementSegment({
      sessionId: record.session.id,
      browserFingerprint: fingerprintValue.identity.userAgent,
      renderer: fingerprintValue.webgl.renderer,
    });
    const invalidSegment = structuredClone(segment);
    invalidSegment.initialSnapshot.transferBytes += 1;
    const invalidResponse = await request({
      port: record.targetPort,
      pathname: "/_pliego/api/segments",
      method: "POST",
      headers: { cookie: record.sessionCookie },
      json: invalidSegment,
    });
    assert.equal(invalidResponse.statusCode, 422);
    assert.match(invalidResponse.json().details.join("\n"), /does not reconcile/);

    const response = await request({
      port: record.targetPort,
      pathname: "/_pliego/api/segments",
      method: "POST",
      headers: { cookie: record.sessionCookie },
      json: segment,
    });
    assert.equal(response.statusCode, 201, response.body.toString("utf8"));
    const receipt = response.json();
    assert.equal(receipt.candidate, true, receipt.violations.join("; "));

    const retryResponse = await request({
      port: record.targetPort,
      pathname: "/_pliego/api/segments",
      method: "POST",
      headers: { cookie: record.sessionCookie },
      json: segment,
    });
    assert.equal(retryResponse.statusCode, 201, retryResponse.body.toString("utf8"));
    assert.deepEqual(retryResponse.json(), receipt);

    const lateSegment = structuredClone(segment);
    lateSegment.segmentId = "fedcba9876543210";
    lateSegment.final = false;
    lateSegment.capturedAt = "2026-07-11T12:00:20.000Z";
    const lateResponse = await request({
      port: record.targetPort,
      pathname: "/_pliego/api/segments",
      method: "POST",
      headers: { cookie: record.sessionCookie },
      json: lateSegment,
    });
    assert.equal(lateResponse.statusCode, 409, lateResponse.body.toString("utf8"));
    assert.deepEqual(lateResponse.json(), { error: "session-finalized" });
    await assert.rejects(
      readFile(
        path.join(
          temporaryRawRoot,
          record.session.id,
          `${lateSegment.segmentId}.json`,
        ),
      ),
      (error) => error.code === "ENOENT",
    );

    const run = JSON.parse(
      await readFile(path.join(temporaryRoot, "inbox", receipt.fileName), "utf8"),
    );
    assert.equal(run.runVersion, "1.1.0");
    assert.equal(run.server.collectorVersion, "pliego-route-lab/0.2.2");
    assert.equal(run.initialRouteResources.length, 2);
    assert.equal(run.observations.transferBytes, 1000);
    assert.equal(run.observations.sessionTransferBytes, 1000);
    assert.equal(run.observations.sessionEncodedBodyBytes, 800);
    assert.equal(run.observations.sessionDecodedBodyBytes, 1600);
    assert.equal(run.observations.resourceCount, 1);
  });
});

function measurementSegment({ sessionId, browserFingerprint, renderer }) {
  const resources = [
    {
      entryType: "navigation",
      scope: "target-origin",
      path: "/",
      initiator: "navigation",
      transferBytes: 300,
      encodedBodyBytes: 200,
      decodedBodyBytes: 400,
      cacheState: "network",
      durationMs: 50,
    },
    {
      entryType: "resource",
      scope: "target-origin",
      path: "/authored.js",
      initiator: "script",
      transferBytes: 700,
      encodedBodyBytes: 600,
      decodedBodyBytes: 1200,
      cacheState: "network",
      durationMs: 20,
    },
  ];
  return {
    segmentVersion: "1.1.0",
    capturedAt: "2026-07-11T12:00:10.000Z",
    sessionId,
    segmentId: "0123456789abcdef",
    final: true,
    pagePath: "/",
    durationMs: 10_500,
    browserFingerprint,
    viewport: { width: 1440, height: 900, devicePixelRatio: 1 },
    initialSnapshot: {
      capturedAt: "2026-07-11T12:00:01.000Z",
      overflowed: false,
      resources,
      transferBytes: 1000,
      encodedBodyBytes: 800,
      decodedBodyBytes: 1600,
      resourceCount: 1,
      cachedResponseCount: 0,
    },
    conditions: {
      foreground: true,
      reducedMotionMatched: false,
      serviceWorkerControlled: false,
    },
    steps: {
      ready: "complete",
      scroll: "not-applicable",
      navigation: "not-applicable",
      visualResponse: "not-applicable",
      disclosure: "not-applicable",
      sceneHold: "not-applicable",
      returnToInitial: "not-applicable",
    },
    metrics: {
      transferBytes: 1000,
      encodedBodyBytes: 800,
      decodedBodyBytes: 1600,
      resourceCount: 1,
      cachedResponseCount: 0,
      decodeDurations: [],
      targetDecodeDurations: [],
      longTaskDurations: [],
      targetMainThreadDurations: [],
      interactionDurations: [],
      estimatedVramBytes: 0,
      drawCalls: null,
      triangles: null,
      drawCallSamples: [],
      triangleSamples: [],
      sceneHook: false,
      activeTier: "universal",
      webglRenderer: renderer,
      hasWebglCanvas: false,
      frameDeltas: [16.6, 16.7, 16.8, 16.9],
      lcpMs: null,
      cls: null,
    },
    capabilities: {
      lcp: false,
      longtask: false,
      eventTiming: false,
      layoutShift: false,
    },
    violations: [],
  };
}

async function startUpstream(targetId) {
  const requests = [];
  const html = Buffer.from(
    `<!doctype html><html><head><meta charset="utf-8"><script src="/authored.js"></script></head><body data-target="${targetId}">${targetId}</body></html>`,
  );
  const compressedHtml = gzipSync(html);
  const server = createServer((request, response) => {
    requests.push({ url: request.url, headers: { ...request.headers } });
    if (request.url === "/echo-cookie") {
      const body = Buffer.from(`${JSON.stringify({ cookie: request.headers.cookie ?? "" })}\n`);
      response.writeHead(200, {
        "Content-Type": "application/json",
        "Content-Length": body.length,
      });
      response.end(body);
      return;
    }

    response.writeHead(200, {
      "Content-Type": "text/html; charset=utf-8",
      "Content-Encoding": "gzip",
      "Content-Length": compressedHtml.length,
      "Cache-Control": "public, max-age=3600",
      "Content-Security-Policy": "default-src 'self'; script-src 'self'; object-src 'none'",
      ETag: '"source-v1"',
      "Set-Cookie": `shared=${targetId}; Path=/; Domain=127.0.0.1; SameSite=Lax`,
    });
    response.end(compressedHtml);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  return {
    targetId,
    server,
    requests,
    html,
    origin: `http://127.0.0.1:${address.port}`,
  };
}

function startRouteLab({ dashboardPort, outputRoot, rawRoot, upstreams, fixture }) {
  const environment = {
    ...process.env,
    PLIEGO_TARGET_ALPHA_URL: upstreams[0].origin,
    PLIEGO_TARGET_BETA_URL: upstreams[1].origin,
    PLIEGO_TARGET_GAMMA_URL: upstreams[2].origin,
  };
  return spawn(
    process.execPath,
    [
      serverPath,
      "--host",
      "127.0.0.1",
      "--port",
      String(dashboardPort),
      "--output",
      outputRoot,
      "--raw-output",
      rawRoot,
      "--plan",
      fixture.plan,
      "--baseline",
      fixture.baseline,
      "--fingerprints",
      fixture.fingerprints,
    ],
    {
      cwd: repoRoot,
      env: environment,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
}

async function writeIntegrationFixture(root) {
  const fingerprints = path.join(root, "fingerprints");
  await mkdir(fingerprints, { recursive: true });
  const fingerprintName = "integration-device.json";
  await writeFile(
    path.join(fingerprints, fingerprintName),
    `${JSON.stringify({
      deviceId: "integration-device",
      role: "engineering-reference",
      capturedAt: "2026-01-01T00:00:00.000Z",
      identity: {
        userAgent: "PliegoRouteLabTest/1.0",
        browserFamily: "Integration test",
      },
      webgl: { renderer: "Deterministic software renderer" },
      viewport: { width: 1440, height: 900, devicePixelRatio: 1 },
    }, null, 2)}\n`,
  );
  const targets = targetIds.map((id, index) => ({
    id,
    baseUrlEnvironment: `PLIEGO_TARGET_${["ALPHA", "BETA", "GAMMA"][index]}_URL`,
    localDefault: `http://127.0.0.1:${6201 + index}`,
    routes: ["/"],
  }));
  const plan = path.join(root, "measurement-plan.json");
  await writeFile(
    plan,
    `${JSON.stringify({
      planVersion: "1.0.0",
      devices: [{
        id: "integration-device",
        role: "engineering-reference",
        status: "ready",
        tiers: ["universal"],
        networkProfile: "desktop-broadband",
        viewport: { mode: "fixed", orientation: "landscape", width: 1440, height: 900, deviceScaleFactor: 1 },
      }],
      targets,
    }, null, 2)}\n`,
  );
  const baseline = path.join(root, "baseline.json");
  await writeFile(
    baseline,
    `${JSON.stringify({
      targets: targetIds.map((id, index) => ({
        work: { id },
        manifestSha256: String(index + 1).repeat(64),
      })),
    }, null, 2)}\n`,
  );
  return { plan, baseline, fingerprints };
}

async function waitForRouteLab(child, dashboardPort, listenerCount) {
  let lastError;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (child.exitCode !== null) {
      const stderr = await streamText(child.stderr);
      throw new Error(`Route Lab exited during startup (${child.exitCode}): ${stderr}`);
    }
    try {
      const results = await Promise.all(
        Array.from({ length: listenerCount }, (_, index) =>
          request({ port: dashboardPort + index, pathname: "/_pliego/health" }),
        ),
      );
      if (results.every((result) => result.statusCode === 200)) return;
    } catch (error) {
      lastError = error;
    }
    await delay(50);
  }
  throw new Error(`Route Lab did not become ready: ${lastError?.message ?? "timeout"}`);
}

async function findConsecutivePorts(count) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const base = 20_000 + Math.floor(Math.random() * 20_000);
    const reservations = [];
    try {
      for (let index = 0; index < count; index += 1) {
        const reservation = createNetServer();
        reservations.push(reservation);
        reservation.listen(base + index, "127.0.0.1");
        await once(reservation, "listening");
      }
      await Promise.all(reservations.map(closeServer));
      return base;
    } catch {
      await Promise.all(reservations.map(closeServer));
    }
  }
  throw new Error(`unable to reserve ${count} consecutive ports`);
}

function request({ port, pathname, method = "GET", headers = {}, json }) {
  const body = json === undefined ? null : Buffer.from(JSON.stringify(json));
  const requestHeaders = { ...headers };
  if (body) {
    requestHeaders["content-type"] = "application/json";
    requestHeaders["content-length"] = String(body.length);
  }
  return new Promise((resolve, reject) => {
    const outgoing = httpRequest(
      {
        hostname: "127.0.0.1",
        port,
        path: pathname,
        method,
        headers: requestHeaders,
      },
      (incoming) => {
        const chunks = [];
        incoming.on("data", (chunk) => chunks.push(chunk));
        incoming.on("end", () => {
          const responseBody = Buffer.concat(chunks);
          resolve({
            statusCode: incoming.statusCode,
            headers: incoming.headers,
            body: responseBody,
            json: () => JSON.parse(responseBody.toString("utf8")),
          });
        });
      },
    );
    outgoing.on("error", reject);
    if (body) outgoing.write(body);
    outgoing.end();
  });
}

function onlyCookie(response) {
  const cookies = response.headers["set-cookie"] ?? [];
  assert.equal(cookies.length, 1);
  return cookies[0].split(";", 1)[0];
}

function cookieWithPrefix(response, prefix) {
  const cookie = (response.headers["set-cookie"] ?? [])
    .map((value) => value.split(";", 1)[0])
    .find((value) => value.startsWith(prefix));
  assert.ok(cookie, `expected a Set-Cookie value beginning with ${prefix}`);
  return cookie;
}

function cookieKey(value) {
  return String(value).replace(/[^a-z0-9]/gi, "_").toLowerCase();
}

async function closeServer(server) {
  if (!server?.listening) return;
  await new Promise((resolve) => server.close(resolve));
}

async function stopChild(child) {
  if (child.exitCode !== null) return;
  const exited = once(child, "exit");
  child.kill("SIGKILL");
  await exited;
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function streamText(stream) {
  const chunks = [];
  for await (const chunk of stream) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}
