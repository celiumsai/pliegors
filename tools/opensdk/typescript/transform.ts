// SPDX-License-Identifier: Apache-2.0

type Input = Readonly<{ source: string; prefix: string }>;

let encoded = "";
for await (const chunk of process.stdin) {
  encoded += chunk;
  if (Buffer.byteLength(encoded, "utf8") > 1024 * 1024) {
    throw new Error("input exceeds 1 MiB");
  }
}
const value: unknown = JSON.parse(encoded);
if (!isInput(value)) throw new Error("input must contain exactly source and prefix strings");
if (value.source.length > 64 * 1024 || value.prefix.length > 1024) {
  throw new Error("transform input exceeds field limits");
}
if (!/^[\x00-\x7f]*$/.test(value.source) || !/^[\x00-\x7f]*$/.test(value.prefix)) {
  throw new Error("uppercase-v1 accepts ASCII input only");
}
const transformed = `${value.prefix}${value.source.toUpperCase()}`;
process.stdout.write(`${JSON.stringify({
  schema: "dev.pliegors.build-transform/v1",
  mediaType: "text/plain; charset=utf-8",
  bytesBase64: Buffer.from(transformed, "utf8").toString("base64"),
})}\n`);

function isInput(value: unknown): value is Input {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  return Object.keys(record).sort().join(",") === "prefix,source" &&
    typeof record.source === "string" && typeof record.prefix === "string";
}
