import { createHash, randomBytes } from "node:crypto";
import { createServer, request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import {
  brotliCompressSync,
  brotliDecompressSync,
  deflateSync,
  gunzipSync,
  gzipSync,
  inflateSync,
  zstdCompressSync,
  zstdDecompressSync,
} from "node:zlib";
import {
  aggregateSegments,
  allowProbeNonce,
  attachReceipt,
  injectProbe,
  rewriteLocation,
  safeFilePart,
  upstreamUrlForRequest,
  validateSegment,
  validateSessionInput,
} from "./core.mjs";

const labRoot = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(labRoot, "..", "..");
const deviceLabRoot = path.resolve(labRoot, "..", "device-lab");
const options = parseArguments(process.argv.slice(2));
const host = options.host ?? "127.0.0.1";
const dashboardPort = Number(options.port ?? 5330);
const outputRoot = path.resolve(
  repoRoot,
  options.output ?? path.join("measurements", "runs", "inbox"),
);
const rawRoot = path.resolve(
  repoRoot,
  options["raw-output"] ?? path.join("measurements", "runs", "raw"),
);
const planPath = path.resolve(
  repoRoot,
  options.plan ?? path.join("fixtures", "phase-1", "measurement-plan.json"),
);
const baselinePath = path.resolve(
  repoRoot,
  options.baseline ?? path.join("fixtures", "phase-1", "baseline.expected.json"),
);
const acceptedRoot = path.resolve(
  repoRoot,
  options.fingerprints ?? path.join("measurements", "accepted"),
);
const plan = JSON.parse(await readFile(planPath, "utf8"));
const baselineBytes = await readFile(baselinePath);
const baseline = JSON.parse(baselineBytes.toString("utf8"));
const contractRevision = `phase-1:${createHash("sha256").update(baselineBytes).digest("hex")}`;
const fingerprints = await loadFingerprints(acceptedRoot);
const sessions = new Map();
const targetPorts = new Map(
  plan.targets.map((target, index) => [target.id, dashboardPort + index + 1]),
);

const staticFiles = new Map([
  ["/_pliego/", [labRoot, "index.html", "text/html; charset=utf-8"]],
  ["/_pliego/index.html", [labRoot, "index.html", "text/html; charset=utf-8"]],
  ["/_pliego/app.js", [labRoot, "app.js", "text/javascript; charset=utf-8"]],
  ["/_pliego/probe-contract.js", [labRoot, "probe-contract.js", "text/javascript; charset=utf-8"]],
  ["/_pliego/probe.js", [labRoot, "probe.js", "text/javascript; charset=utf-8"]],
  ["/_pliego/styles.css", [labRoot, "styles.css", "text/css; charset=utf-8"]],
  ["/_pliego/pliego-mark.svg", [deviceLabRoot, "pliego-mark.svg", "image/svg+xml"]],
  ["/_pliego/instrument-sans.woff2", [deviceLabRoot, "instrument-sans.woff2", "font/woff2"]],
  ["/_pliego/instrument-serif-italic.woff2", [deviceLabRoot, "instrument-serif-italic.woff2", "font/woff2"]],
  ["/_pliego/fragment-mono.woff2", [deviceLabRoot, "fragment-mono.woff2", "font/woff2"]],
]);

await mkdir(outputRoot, { recursive: true });
await mkdir(rawRoot, { recursive: true });

const listeners = [
  { kind: "dashboard", port: dashboardPort, target: null },
  ...plan.targets.map((target) => ({
    kind: "target",
    port: targetPorts.get(target.id),
    target,
  })),
];

for (const context of listeners) {
  const server = createServer((request, response) => handleRequest(request, response, context));
  server.listen(context.port, host, () => {
    const label = context.target ? ` / ${context.target.id}` : " / dashboard";
    process.stdout.write(`PLIEGO Route Lab listening on http://${host}:${context.port}${label}\n`);
  });
}
process.stdout.write(`Raw route runs: ${outputRoot}\n`);

async function handleRequest(request, response, context) {
  try {
    const url = new URL(request.url ?? "/", `http://${request.headers.host}`);
    if (url.pathname.startsWith("/_pliego/")) {
      return await handleLabRequest(request, response, url, context);
    }

    if (context.kind !== "target") return redirectToDashboard(request, response);
    const session = sessionFromRequest(request, context.target.id);
    if (!session || session.targetId !== context.target.id) {
      return redirectToDashboard(request, response);
    }
    return proxyTarget(request, response, session);
  } catch (error) {
    process.stderr.write(`${error.stack ?? error}\n`);
    if (!response.headersSent) {
      return sendJson(response, error.statusCode ?? 500, {
        error: error.statusCode ? "request-error" : "server-error",
        detail: error.message,
      });
    }
    response.end();
  }
}

async function handleLabRequest(request, response, url, context) {
  setLabSecurityHeaders(response);
  if (request.method === "GET" && url.pathname === "/_pliego/health") {
    return sendJson(response, 200, {
      healthy: true,
      service: "pliego-route-lab",
      listener: context.target?.id ?? "dashboard",
      activeSessions: sessions.size,
    });
  }
  if (
    context.kind === "dashboard" &&
    request.method === "GET" &&
    url.pathname === "/_pliego/api/config"
  ) {
    const inboxFiles = await listJson(outputRoot);
    return sendJson(response, 200, {
      planVersion: plan.planVersion,
      contractRevision,
      devices: plan.devices.map((device) => ({
        id: device.id,
        role: device.role,
        status: device.status,
        tiers: device.tiers,
        networkProfile: device.networkProfile,
      })),
      targets: plan.targets.map((target) => ({
        id: target.id,
        routes: target.routes,
        endpoint: targetEndpoint(target),
        measurementPort: targetPorts.get(target.id),
      })),
      fingerprints: fingerprints.map((fingerprint) => ({
        fileName: fingerprint.fileName,
        deviceId: fingerprint.deviceId,
        role: fingerprint.role,
        capturedAt: fingerprint.capturedAt,
        browser: fingerprint.browser,
        renderer: fingerprint.renderer,
      })),
      inboxRunCount: inboxFiles.length,
    });
  }
  if (
    context.kind === "target" &&
    request.method === "GET" &&
    url.pathname === "/_pliego/launch"
  ) {
    const session = sessions.get(url.searchParams.get("session")) ?? null;
    if (!session || session.targetId !== context.target.id) {
      return sendJson(response, 404, { error: "session-not-found" });
    }
    response.setHeader(
      "Set-Cookie",
      `${sessionCookieName(session.targetId)}=${session.id}; Path=/; HttpOnly; SameSite=Strict`,
    );
    if (session.cacheMode === "cold") response.setHeader("Clear-Site-Data", '"cache"');
    const launchRoute = new URL(session.route, "http://route-lab.invalid");
    launchRoute.searchParams.set("__pliego_run", session.id);
    response.writeHead(302, {
      Location: `${launchRoute.pathname}${launchRoute.search}`,
      "Cache-Control": "no-store",
    });
    return response.end();
  }
  if (
    context.kind === "target" &&
    request.method === "GET" &&
    url.pathname === "/_pliego/api/session"
  ) {
    const session = sessionFromRequest(request, context.target.id);
    if (!session || session.targetId !== context.target.id) {
      return sendJson(response, 404, { error: "session-not-found" });
    }
    return sendJson(response, 200, publicSession(session));
  }
  if (
    context.kind === "dashboard" &&
    request.method === "POST" &&
    url.pathname === "/_pliego/api/sessions"
  ) {
    const input = await readJson(request, 128 * 1024);
    const result = validateSessionInput(input, plan, fingerprints);
    if (result.errors.length) {
      return sendJson(response, 422, {
        error: "invalid-session",
        details: result.errors,
      });
    }
    const id = randomBytes(12).toString("hex");
    const target = plan.targets.find((candidate) => candidate.id === result.value.targetId);
    const targetBase = targetEndpoint(target);
    const fingerprint = fingerprints.find(
      (candidate) => candidate.fileName === result.value.hardwareFingerprint,
    );
    const manifestSha256 = baseline.targets.find(
      (candidate) => candidate.work.id === result.value.targetId,
    )?.manifestSha256;
    if (!manifestSha256) {
      return sendJson(response, 500, { error: "target-manifest-fingerprint-missing" });
    }
    pruneSessions();
    const session = {
      id,
      ...result.value,
      hardwareFingerprint: `${fingerprint.fileName}#sha256=${fingerprint.sha256}`,
      expectedUserAgent: fingerprint.userAgent,
      probeNonce: randomBytes(18).toString("base64"),
      sourceRevision: `pending;manifest-sha256:${manifestSha256}`,
      manifestSha256,
      targetBase,
      targetPort: targetPorts.get(result.value.targetId),
      dashboardOrigin: originForPort(url.hostname, dashboardPort),
      expectedRenderer: fingerprint.renderer,
      expectedFingerprintViewport: fingerprint.viewport,
      expectedViewport: plan.devices.find(
        (device) => device.id === result.value.deviceId,
      ).viewport,
      cacheControl:
        result.value.cacheMode === "cold"
          ? "clear-site-data-requested"
          : "warm-cache-preserved",
      createdAt: new Date().toISOString(),
      segments: new Map(),
      receipt: null,
    };
    sessions.set(id, session);
    return sendJson(response, 201, {
      session: publicSession(session),
      launchUrl: `${originForPort(url.hostname, session.targetPort)}/_pliego/launch?session=${id}`,
    });
  }
  if (
    context.kind === "target" &&
    request.method === "POST" &&
    url.pathname === "/_pliego/api/segments"
  ) {
    const session = sessionFromRequest(request, context.target.id);
    if (!session || session.targetId !== context.target.id) {
      return sendJson(response, 404, { error: "session-not-found" });
    }
    const segment = await readJson(request, 1024 * 1024);
    const errors = validateSegment(segment, session);
    if (errors.length) {
      return sendJson(response, 422, { error: "invalid-segment", details: errors });
    }
    if (session.receipt) {
      const persisted = session.segments.get(segment.segmentId);
      if (persisted?.final && segment.final && isDeepStrictEqual(segment, persisted)) {
        return sendJson(response, 201, session.receipt);
      }
      return sendJson(response, 409, { error: "session-finalized" });
    }
    if (!session.segments.has(segment.segmentId) && session.segments.size >= 32) {
      return sendJson(response, 422, {
        error: "segment-limit-exceeded",
        details: ["a route run may contain at most 32 document segments"],
      });
    }
    if (!session.segments.has(segment.segmentId)) {
      session.segments.set(segment.segmentId, segment);
      await persistSegment(session, segment);
    }
    if (!segment.final) {
      return sendJson(response, 202, { saved: true, segmentId: segment.segmentId });
    }
    if (!session.receipt) session.receipt = await finalizeSession(session);
    return sendJson(response, 201, session.receipt);
  }
  if (
    context.kind === "target" &&
    request.method === "POST" &&
    url.pathname === "/_pliego/api/end"
  ) {
    response.setHeader(
      "Set-Cookie",
      `${sessionCookieName(context.target.id)}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0`,
    );
    return sendJson(response, 200, { ended: true });
  }
  if (
    context.kind === "target" &&
    request.method === "GET" &&
    (url.pathname === "/_pliego/" || url.pathname === "/_pliego/index.html")
  ) {
    response.writeHead(302, { Location: originForPort(url.hostname, dashboardPort) + "/_pliego/" });
    return response.end();
  }
  if (request.method === "GET" && staticFiles.has(url.pathname)) {
    const [root, fileName, contentType] = staticFiles.get(url.pathname);
    const content = await readFile(path.join(root, fileName));
    response.writeHead(200, {
      "Content-Type": contentType,
      "Content-Length": content.length,
      "Cache-Control": "no-store",
    });
    return response.end(content);
  }
  return sendJson(response, 404, { error: "not-found" });
}

async function finalizeSession(session) {
  const capturedAt = new Date().toISOString();
  const run = aggregateSegments(session, [...session.segments.values()], capturedAt);
  const record = attachReceipt(run, capturedAt);
  const day = capturedAt.slice(0, 10);
  const routePart = safeFilePart(session.route) || "root";
  const fileName = [
    day,
    safeFilePart(session.deviceId),
    safeFilePart(session.targetId),
    routePart,
    session.tier,
    session.cacheMode,
    session.id.slice(0, 8),
  ].join("-") + ".json";
  await writeFile(path.join(outputRoot, fileName), `${JSON.stringify(record, null, 2)}\n`, {
    encoding: "utf8",
    flag: "wx",
  });
  return {
    saved: true,
    fileName,
    sha256: record.server.sha256,
    candidate: record.violations.length === 0,
    violations: record.violations,
    segmentCount: session.segments.size,
  };
}

async function persistSegment(session, segment) {
  const directory = path.join(rawRoot, session.id);
  await mkdir(directory, { recursive: true });
  await writeFile(
    path.join(directory, `${segment.segmentId}.json`),
    `${JSON.stringify(segment, null, 2)}\n`,
    { encoding: "utf8", flag: "wx" },
  );
}

function proxyTarget(request, response, session) {
  const upstreamUrl = upstreamUrlForRequest(request.url ?? "/", session.targetBase);
  const isRunBootstrap = upstreamUrl.searchParams.has("__pliego_run");
  upstreamUrl.searchParams.delete("__pliego_session");
  upstreamUrl.searchParams.delete("__pliego_run");
  const headers = { ...request.headers };
  headers.host = upstreamUrl.host;
  headers.cookie = forwardTargetCookies(headers.cookie, session.targetId);
  if (!headers.cookie) delete headers.cookie;
  if (isRunBootstrap) {
    delete headers["if-none-match"];
    delete headers["if-modified-since"];
    headers["cache-control"] = "no-cache";
  }
  rewriteRequestOrigin(headers, session.targetBase);
  delete headers.connection;
  delete headers["content-length"];

  const requestUpstream = upstreamUrl.protocol === "https:" ? httpsRequest : httpRequest;
  if (!new Set(["http:", "https:"]).has(upstreamUrl.protocol)) {
    return sendJson(response, 502, { error: "unsupported-upstream-protocol" });
  }
  const upstream = requestUpstream(
    upstreamUrl,
    {
      method: request.method,
      headers,
    },
    (upstreamResponse) => {
      const contentType = String(upstreamResponse.headers["content-type"] ?? "");
      if (contentType.includes("text/html")) {
        proxyHtml(upstreamResponse, response, session, upstreamUrl).catch((error) => {
          process.stderr.write(`${error.stack ?? error}\n`);
          if (!response.headersSent) {
            sendJson(response, error.statusCode ?? 502, {
              error: "upstream-html-error",
              detail: error.message,
            });
          } else {
            response.destroy(error);
          }
        });
        return;
      }
      const responseHeaders = proxyHeaders(upstreamResponse.headers, session);
      response.writeHead(upstreamResponse.statusCode ?? 502, responseHeaders);
      upstreamResponse.pipe(response);
    },
  );
  upstream.on("error", (error) => {
    if (!response.headersSent) {
      sendJson(response, 502, { error: "upstream-unavailable", detail: error.message });
    } else {
      response.destroy(error);
    }
  });
  request.pipe(upstream);
}

async function proxyHtml(upstreamResponse, response, session, upstreamUrl) {
  const chunks = [];
  let total = 0;
  for await (const chunk of upstreamResponse) {
    total += chunk.length;
    if (total > 10 * 1024 * 1024) {
      const error = new Error("upstream HTML exceeds 10 MiB");
      error.statusCode = 502;
      throw error;
    }
    chunks.push(chunk);
  }
  const encoding = String(upstreamResponse.headers["content-encoding"] ?? "identity").toLowerCase();
  const decoded = decodeBody(Buffer.concat(chunks), encoding);
  const originalHtml = decoded.toString("utf8");
  if (
    normalizeRoutePath(upstreamUrl.pathname) === normalizeRoutePath(session.route) &&
    session.sourceRevision.startsWith("pending;")
  ) {
    const htmlSha256 = createHash("sha256").update(decoded).digest("hex");
    session.sourceRevision =
      `html-sha256:${htmlSha256};manifest-sha256:${session.manifestSha256}`;
  }
  const instrumented = Buffer.from(
    injectProbe(originalHtml, session.probeNonce, {
      tier: session.tier,
      motionMode: session.motionMode,
    }),
  );
  const body = encodeBody(instrumented, encoding);
  const responseHeaders = proxyHeaders(upstreamResponse.headers, session);
  delete responseHeaders["content-length"];
  delete responseHeaders.etag;
  responseHeaders["cache-control"] = "no-store";
  responseHeaders["content-length"] = String(body.length);
  responseHeaders["x-pliego-route-lab"] = "0.2.2";
  if (responseHeaders["content-security-policy"]) {
    responseHeaders["content-security-policy"] = allowProbeNonce(
      responseHeaders["content-security-policy"],
      session.probeNonce,
    );
  }
  if (encoding === "identity") delete responseHeaders["content-encoding"];
  response.writeHead(upstreamResponse.statusCode ?? 200, responseHeaders);
  response.end(body);
}

function proxyHeaders(upstreamHeaders, session) {
  const result = {};
  for (const [name, value] of Object.entries(upstreamHeaders)) {
    const lower = name.toLowerCase();
    if (
      value === undefined ||
      ["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade"].includes(lower)
    ) {
      continue;
    }
    result[lower] =
      lower === "set-cookie" ? rewriteSetCookies(value, session.targetId) : value;
  }
  if (result.location) result.location = rewriteLocation(result.location, session.targetBase);
  if (session.cacheMode === "cold") result["cache-control"] = "no-store";
  return result;
}

function sessionFromRequest(request, targetId) {
  const id = parseCookies(request.headers.cookie)[sessionCookieName(targetId)];
  return id ? sessions.get(id) ?? null : null;
}

function publicSession(session) {
  return {
    id: session.id,
    deviceId: session.deviceId,
    hardwareFingerprint: session.hardwareFingerprint,
    targetId: session.targetId,
    route: session.route,
    tier: session.tier,
    cacheMode: session.cacheMode,
    motionMode: session.motionMode,
    orientation: session.orientation,
    networkProfile: session.networkProfile,
    powerState: session.powerState,
    thermalState: session.thermalState,
    sourceRevision: session.sourceRevision,
    dashboardUrl: `${session.dashboardOrigin}/_pliego/`,
    createdAt: session.createdAt,
  };
}

async function loadFingerprints(directory) {
  const records = [];
  for (const fileName of await listJson(directory)) {
    const bytes = await readFile(path.join(directory, fileName));
    const value = JSON.parse(bytes.toString("utf8"));
    records.push({
      fileName,
      sha256: createHash("sha256").update(bytes).digest("hex"),
      deviceId: value.deviceId,
      role: value.role,
      capturedAt: value.capturedAt,
      browser: value.identity?.browserFamily ?? browserFromUserAgent(value.identity?.userAgent),
      renderer: value.webgl?.renderer ?? "unavailable",
      userAgent: value.identity?.userAgent ?? "",
      viewport: value.viewport,
    });
  }
  return records;
}

function pruneSessions() {
  const cutoff = Date.now() - 4 * 60 * 60 * 1000;
  for (const [id, session] of sessions) {
    if (Date.parse(session.createdAt) < cutoff) sessions.delete(id);
  }
  while (sessions.size >= 128) {
    sessions.delete(sessions.keys().next().value);
  }
}

function redirectToDashboard(request, response) {
  const url = new URL(request.url ?? "/", `http://${request.headers.host}`);
  response.writeHead(302, {
    Location: `${originForPort(url.hostname, dashboardPort)}/_pliego/`,
    "Cache-Control": "no-store",
  });
  response.end();
}

function originForPort(hostname, targetPort) {
  const hostValue = hostname.includes(":") && !hostname.startsWith("[")
    ? `[${hostname}]`
    : hostname;
  return `http://${hostValue}:${targetPort}`;
}

function normalizeRoutePath(value) {
  return String(value).replace(/\/$/, "") || "/";
}

function targetEndpoint(target) {
  return process.env[target.baseUrlEnvironment] ?? target.localDefault;
}

function browserFromUserAgent(userAgent = "") {
  const match = userAgent.match(/(?:Chrome|CriOS)\/([\d.]+)/i);
  if (match) return `Chrome ${match[1]}`;
  const safari = userAgent.match(/Version\/([\d.]+).*Safari\//i);
  if (safari) return `Safari ${safari[1]}`;
  return "unknown";
}

async function listJson(directory) {
  try {
    return (await readdir(directory)).filter((fileName) => fileName.endsWith(".json")).sort();
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }
}

function parseCookies(header = "") {
  return Object.fromEntries(
    header
      .split(";")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const index = part.indexOf("=");
        return index === -1
          ? [part, ""]
          : [part.slice(0, index), part.slice(index + 1)];
      }),
  );
}

function forwardTargetCookies(header = "", targetId) {
  const prefix = targetCookiePrefix(targetId);
  const forwarded = [];
  for (const [name, value] of Object.entries(parseCookies(header))) {
    if (name.startsWith("pliego_route_session_") || name.startsWith("pliego_target_")) {
      if (name.startsWith(prefix)) forwarded.push(`${name.slice(prefix.length)}=${value}`);
      continue;
    }
  }
  return forwarded.join("; ");
}

function rewriteRequestOrigin(headers, targetBase) {
  const base = new URL(targetBase);
  if (headers.origin) headers.origin = base.origin;
  if (headers.referer) {
    try {
      const incoming = new URL(headers.referer);
      const rewritten = new URL(base);
      rewritten.pathname = incoming.pathname;
      rewritten.search = incoming.search;
      rewritten.hash = "";
      headers.referer = rewritten.href;
    } catch {
      delete headers.referer;
    }
  }
}

function rewriteSetCookies(value, targetId) {
  const values = Array.isArray(value) ? value : [value];
  return values
    .map((cookie) => String(cookie))
    .filter((cookie) => !cookie.startsWith("pliego_route_session_"))
    .map((cookie) => {
      const separator = cookie.indexOf("=");
      if (separator <= 0) return null;
      const name = cookie.slice(0, separator);
      const rest = cookie.slice(separator + 1).replace(/;\s*Domain=[^;]*/gi, "");
      return `${targetCookiePrefix(targetId)}${name}=${rest}`;
    })
    .filter(Boolean);
}

function decodeBody(body, encoding) {
  if (encoding === "identity" || !encoding) return body;
  if (encoding === "gzip") return gunzipSync(body);
  if (encoding === "br") return brotliDecompressSync(body);
  if (encoding === "deflate") return inflateSync(body);
  if (encoding === "zstd") return zstdDecompressSync(body);
  const error = new Error(`unsupported upstream content encoding: ${encoding}`);
  error.statusCode = 502;
  throw error;
}

function encodeBody(body, encoding) {
  if (encoding === "identity" || !encoding) return body;
  if (encoding === "gzip") return gzipSync(body);
  if (encoding === "br") return brotliCompressSync(body);
  if (encoding === "deflate") return deflateSync(body);
  if (encoding === "zstd") return zstdCompressSync(body);
  const error = new Error(`unsupported upstream content encoding: ${encoding}`);
  error.statusCode = 502;
  throw error;
}

function sessionCookieName(targetId) {
  return `pliego_route_session_${cookieKey(targetId)}`;
}

function targetCookiePrefix(targetId) {
  return `pliego_target_${cookieKey(targetId)}_`;
}

function cookieKey(value) {
  return String(value).replace(/[^a-z0-9]/gi, "_").toLowerCase();
}

async function readJson(request, maximumBytes) {
  const chunks = [];
  let total = 0;
  for await (const chunk of request) {
    total += chunk.length;
    if (total > maximumBytes) {
      const error = new Error("request body too large");
      error.statusCode = 413;
      throw error;
    }
    chunks.push(chunk);
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
  } catch {
    const error = new Error("invalid JSON");
    error.statusCode = 400;
    throw error;
  }
}

function sendJson(response, status, value) {
  const body = Buffer.from(`${JSON.stringify(value)}\n`);
  response.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": body.length,
    "Cache-Control": "no-store",
  });
  response.end(body);
}

function setLabSecurityHeaders(response) {
  response.setHeader("X-Content-Type-Options", "nosniff");
  response.setHeader("Referrer-Policy", "no-referrer");
  response.setHeader("X-Frame-Options", "DENY");
  response.setHeader(
    "Content-Security-Policy",
    "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'",
  );
}

function parseArguments(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const option = values[index];
    const value = values[index + 1];
    if (!option?.startsWith("--") || !value) {
      throw new Error(
        "Expected a supported option followed by a value",
      );
    }
    const name = option.slice(2);
    if (!new Set(["host", "port", "output", "raw-output", "plan", "baseline", "fingerprints"]).has(name)) {
      throw new Error(`Unknown option: --${name}`);
    }
    parsed[name] = value;
  }
  return parsed;
}
