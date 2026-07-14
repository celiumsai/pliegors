#!/usr/bin/env node

import { access, readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const rootMarkdown = (await readdir(root, { withFileTypes: true }))
  .filter((entry) => entry.isFile() && entry.name.endsWith(".md"))
  .map((entry) => path.join(root, entry.name));
const markdown = [
  ...rootMarkdown,
  ...(await walk(path.join(root, "brand"))),
  ...(await walk(path.join(root, "docs"))),
  ...(await walk(path.join(root, "workers"))),
].filter((file) => file.endsWith(".md"));
const failures = [];

for (const file of markdown) {
  const source = await readFile(file, "utf8");
  for (const match of source.matchAll(/!?\[[^\]]*\]\(([^)]+)\)/g)) {
    const raw = match[1].trim().replace(/^<|>$/g, "");
    if (!raw || /^(?:[a-z]+:|#)/i.test(raw)) continue;
    const target = decodeURIComponent(raw.split("#", 1)[0]);
    const absolute = path.resolve(path.dirname(file), target);
    try {
      await access(absolute);
    } catch {
      failures.push(`${path.relative(root, file)} -> ${raw}`);
    }
  }
}

if (failures.length > 0) {
  throw new Error(`broken local documentation links:\n${failures.join("\n")}`);
}
process.stdout.write(`documentation links PASS: ${markdown.length} Markdown files\n`);

async function walk(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory() && !["node_modules", "target", ".wrangler"].includes(entry.name)) {
      files.push(...(await walk(absolute)));
    }
    else if (entry.isFile()) files.push(absolute);
  }
  return files;
}
