#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";
import { parse } from "parse5";

const projects = process.argv.slice(2).map((value) => path.resolve(value));
if (projects.length === 0) {
  throw new Error("usage: node scripts/check-starter-builds.mjs <project> [...project]");
}

for (const project of projects) {
  const output = path.join(project, "target", "site");
  const ledgerPath = path.join(output, "pliego.build.json");
  const ledger = JSON.parse(await readFile(ledgerPath, "utf8"));
  if (ledger.reportVersion !== "1.0.0" || !Array.isArray(ledger.files)) {
    throw new Error(`${project}: invalid PliegoRS build ledger`);
  }

  const tracked = new Set();
  for (const file of ledger.files) {
    const absolute = path.join(output, ...file.path.split("/"));
    const bytes = await readFile(absolute);
    const digest = createHash("sha256").update(bytes).digest("hex");
    if (bytes.length !== file.bytes || digest !== file.sha256) {
      throw new Error(`${project}: ledger mismatch for ${file.path}`);
    }
    tracked.add(file.path);
  }

  const emitted = (await walk(output))
    .map((file) => path.relative(output, file).split(path.sep).join("/"))
    .filter((file) => file !== "pliego.build.json");
  for (const file of emitted) {
    if (!tracked.has(file)) throw new Error(`${project}: untracked output ${file}`);
  }
  for (const file of tracked) {
    if (!emitted.includes(file)) throw new Error(`${project}: missing output ${file}`);
  }

  const htmlFiles = emitted.filter((file) => file.endsWith(".html"));
  if (!htmlFiles.includes("index.html") || !htmlFiles.includes("404.html")) {
    throw new Error(`${project}: starter must emit index.html and 404.html`);
  }
  for (const file of htmlFiles) {
    const source = await readFile(path.join(output, file), "utf8");
    rejectFormerRuntimes(project, file, source);
    const document = parse(source);
    const attributes = collectAttributes(document);
    requireMetadata(project, file, attributes);
    for (const reference of localReferences(attributes)) {
      if (!(await referenceExists(output, file, reference))) {
        throw new Error(`${project}: ${file} references missing ${reference}`);
      }
    }
  }

  for (const file of emitted.filter((file) => file.endsWith(".css"))) {
    const source = await readFile(path.join(output, file), "utf8");
    for (const match of source.matchAll(/url\((['"]?)(.*?)\1\)/g)) {
      const reference = match[2].trim();
      if (!isLocal(reference) || reference.startsWith("data:")) continue;
      if (!(await referenceExists(output, file, reference))) {
        throw new Error(`${project}: ${file} references missing ${reference}`);
      }
    }
  }

  const totalBytes = ledger.files.reduce((sum, file) => sum + file.bytes, 0);
  process.stdout.write(
    `starter PASS ${path.basename(project)}: ${htmlFiles.length} routes / ${ledger.files.length} files / ${totalBytes} bytes\n`,
  );
}

async function walk(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(absolute)));
    else if (entry.isFile()) files.push(absolute);
  }
  return files;
}

function collectAttributes(node, output = []) {
  if (node.tagName) {
    output.push({
      tag: node.tagName,
      attrs: Object.fromEntries((node.attrs ?? []).map(({ name, value }) => [name, value])),
    });
  }
  for (const child of node.childNodes ?? []) collectAttributes(child, output);
  return output;
}

function requireMetadata(project, file, elements) {
  const has = (tag, predicate) => elements.some((element) => element.tag === tag && predicate(element.attrs));
  const requirements = [
    ["generator", has("meta", (attrs) => attrs.name === "generator" && attrs.content === "PliegoRS")],
    ["canonical", has("link", (attrs) => attrs.rel === "canonical")],
    ["manifest", has("link", (attrs) => attrs.rel === "manifest")],
    ["touch icon", has("link", (attrs) => attrs.rel === "apple-touch-icon")],
    ["Open Graph", has("meta", (attrs) => attrs.property?.startsWith("og:"))],
  ];
  for (const [label, present] of requirements) {
    if (!present) throw new Error(`${project}: ${file} lacks ${label} metadata`);
  }
}

function localReferences(elements) {
  const references = [];
  for (const { attrs } of elements) {
    for (const name of ["src", "href", "poster"]) {
      if (isLocal(attrs[name])) references.push(attrs[name]);
    }
    for (const candidate of (attrs.srcset ?? "").split(",")) {
      const reference = candidate.trim().split(/\s+/)[0];
      if (isLocal(reference)) references.push(reference);
    }
  }
  return references;
}

function isLocal(reference) {
  return Boolean(reference) && !/^(?:[a-z]+:|#|\/\/)/i.test(reference);
}

function resolveReference(output, sourceFile, reference) {
  const pathname = new URL(reference, "https://pliego.invalid/").pathname;
  if (pathname === "/") return path.join(output, "index.html");
  if (pathname.endsWith("/")) return path.join(output, pathname.slice(1), "index.html");
  if (reference.startsWith("/")) return path.join(output, ...pathname.slice(1).split("/"));
  return path.resolve(path.dirname(path.join(output, sourceFile)), ...pathname.split("/"));
}

async function referenceExists(output, sourceFile, reference) {
  const target = resolveReference(output, sourceFile, reference);
  if (await exists(target)) return true;
  const pathname = new URL(reference, "https://pliego.invalid/").pathname;
  return path.extname(pathname) === "" && exists(path.join(target, "index.html"));
}

function rejectFormerRuntimes(project, file, source) {
  const marker = /(?:data-astro|astro-island|\bVite\b|__NEXT_DATA__|\bReact\b|\bSvelte\b)/i.exec(source);
  if (marker) throw new Error(`${project}: ${file} contains former runtime marker ${marker[0]}`);
}

async function exists(file) {
  try {
    return (await stat(file)).isFile();
  } catch {
    return false;
  }
}
