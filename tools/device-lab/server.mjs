import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const labRoot = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(labRoot, "..", "..");
const options = parseArguments(process.argv.slice(2));
const host = options.host ?? "127.0.0.1";
const port = Number(options.port ?? 5310);
const outputRoot = path.resolve(
  repoRoot,
  options.output ?? path.join("measurements", "inbox"),
);
const staticFiles = new Map([
  ["/", ["index.html", "text/html; charset=utf-8"]],
  ["/index.html", ["index.html", "text/html; charset=utf-8"]],
  ["/app.js", ["app.js", "text/javascript; charset=utf-8"]],
  ["/styles.css", ["styles.css", "text/css; charset=utf-8"]],
  ["/pliego-mark.svg", ["pliego-mark.svg", "image/svg+xml"]],
  ["/instrument-sans.woff2", ["instrument-sans.woff2", "font/woff2"]],
  ["/instrument-serif-italic.woff2", ["instrument-serif-italic.woff2", "font/woff2"]],
  ["/fragment-mono.woff2", ["fragment-mono.woff2", "font/woff2"]],
]);

await mkdir(outputRoot, { recursive: true });

const server = createServer(async (request, response) => {
  try {
    setSecurityHeaders(response);
    const url = new URL(request.url ?? "/", `http://${request.headers.host}`);
    if (request.method === "GET" && url.pathname === "/health") {
      return sendJson(response, 200, { healthy: true, service: "pliego-device-lab" });
    }
    if (request.method === "POST" && url.pathname === "/api/fingerprints") {
      return await saveFingerprint(request, response);
    }
    if (request.method === "GET" && staticFiles.has(url.pathname)) {
      const [fileName, contentType] = staticFiles.get(url.pathname);
      const content = await readFile(path.join(labRoot, fileName));
      response.writeHead(200, {
        "Content-Type": contentType,
        "Content-Length": content.length,
        "Cache-Control": "no-store",
      });
      return response.end(content);
    }
    sendJson(response, 404, { error: "not-found" });
  } catch (error) {
    process.stderr.write(`${error.stack ?? error}\n`);
    if (!response.headersSent) sendJson(response, 500, { error: "server-error" });
    else response.end();
  }
});

server.listen(port, host, () => {
  process.stdout.write(`PLIEGO Device Lab listening on http://${host}:${port}\n`);
  process.stdout.write(`Captures: ${outputRoot}\n`);
});

async function saveFingerprint(request, response) {
  const raw = await readBody(request, 256 * 1024);
  let payload;
  try {
    payload = JSON.parse(raw.toString("utf8"));
  } catch {
    return sendJson(response, 400, { error: "invalid-json" });
  }
  const validationError = validateFingerprint(payload);
  if (validationError) {
    return sendJson(response, 422, { error: "invalid-fingerprint", detail: validationError });
  }
  const receivedAt = new Date().toISOString();
  const record = {
    ...payload,
    server: {
      receivedAt,
      transport: "local-network",
    },
  };
  const serialized = `${JSON.stringify(record, null, 2)}\n`;
  const digest = createHash("sha256").update(serialized).digest("hex");
  const day = receivedAt.slice(0, 10);
  const fileName = `${day}-${payload.deviceId}-${digest.slice(0, 12)}.json`;
  await writeFile(path.join(outputRoot, fileName), serialized, {
    encoding: "utf8",
    flag: "wx",
  });
  return sendJson(response, 201, { saved: true, fileName, sha256: digest });
}

function validateFingerprint(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return "body must be an object";
  if (value.reportVersion !== "1.0.0") return "reportVersion must be 1.0.0";
  if (!/^[a-z0-9]+(?:[.-][a-z0-9]+)*$/.test(value.deviceId ?? "")) {
    return "deviceId is invalid";
  }
  if (
    !new Set([
      "modest-android",
      "ipad-safari",
      "integrated-gpu-laptop",
      "capable-desktop",
      "engineering-reference",
    ]).has(value.role)
  ) {
    return "role is invalid";
  }
  if (!value.capturedAt || Number.isNaN(Date.parse(value.capturedAt))) {
    return "capturedAt is invalid";
  }
  if (!value.identity || !value.viewport || !value.preferences || !value.frameProbe) {
    return "identity, viewport, preferences, and frameProbe are required";
  }
  if (
    value.role === "ipad-safari" &&
    (!/Version\/[\d.]+.*Safari\//i.test(value.identity.userAgent ?? "") ||
      /CriOS|FxiOS|EdgiOS/i.test(value.identity.userAgent ?? ""))
  ) {
    return "the ipad-safari row requires Safari, not another iPad browser";
  }
  if (!Number.isFinite(value.frameProbe.p95Ms)) return "frameProbe.p95Ms is invalid";
  return null;
}

async function readBody(request, maximumBytes) {
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
  return Buffer.concat(chunks);
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

function setSecurityHeaders(response) {
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
      throw new Error("Expected --host, --port, or --output followed by a value");
    }
    parsed[option.slice(2)] = value;
  }
  return parsed;
}
