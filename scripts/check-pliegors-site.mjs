import { readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { parse } from "parse5";

const root = path.resolve(process.argv[2] ?? "examples/pliegors-site/target/site");
const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const routes = [
  "/",
  "/about",
  "/docs",
  "/docs/getting-started",
  "/docs/project-structure",
  "/docs/cli",
  "/docs/developer-loop",
  "/docs/routing-and-pages",
  "/docs/views",
  "/docs/events-and-folds",
  "/docs/schemas-and-snapshots",
  "/docs/hyphae-sync",
  "/docs/content",
  "/docs/browser-runtime",
  "/docs/dom-lifecycle",
  "/docs/assets",
  "/docs/artifact-trust",
  "/docs/errors-and-diagnostics",
  "/docs/build-and-deploy",
  "/docs/crate-reference",
  "/docs/licensing",
  "/changelog",
  "/security",
  "/accessibility",
  "/legal",
  "/legal/terms",
  "/legal/privacy",
  "/legal/cookies",
  "/legal/acceptable-use",
];
const expected = ["/404.html", ...routes, ...routes.map((route) => route === "/" ? "/es" : `/es${route}`)];
const forbidden = [
  "pliego" + ".run",
  "pliego" + "css",
  "/acquire",
  "/account",
  "one-of-one",
  "subscription",
  "/cases",
  "/es/cases",
];
const failures = [];

const outputPath = (route) => route === "/404.html"
  ? path.join(root, "404.html")
  : route === "/"
    ? path.join(root, "index.html")
    : path.join(root, route.slice(1), "index.html");

const elements = (node, tagName, output = []) => {
  if (node?.tagName && (tagName === "*" || node.tagName === tagName)) output.push(node);
  for (const child of node?.childNodes ?? []) elements(child, tagName, output);
  return output;
};
const attribute = (node, name) => node?.attrs?.find((item) => item.name === name)?.value;

for (const route of expected) {
  const file = outputPath(route);
  let html;
  try {
    html = await readFile(file, "utf8");
  } catch {
    failures.push(`${route}: missing output ${path.relative(root, file)}`);
    continue;
  }
  const lower = html.toLowerCase();
  for (const term of forbidden) {
    if (lower.includes(term)) failures.push(`${route}: forbidden product trace ${term}`);
  }
  const document = parse(html);
  const title = elements(document, "title")[0];
  const canonical = elements(document, "link").find((node) => attribute(node, "rel") === "canonical");
  const alternates = elements(document, "link").filter((node) => attribute(node, "rel") === "alternate");
  const description = elements(document, "meta").find((node) => attribute(node, "name") === "description");
  const robots = elements(document, "meta").find((node) => attribute(node, "name") === "robots");
  const jsonLd = elements(document, "script").find((node) => attribute(node, "type") === "application/ld+json");
  if (!title?.childNodes?.[0]?.value?.trim()) failures.push(`${route}: missing title`);
  if (!attribute(canonical, "href")?.startsWith("https://pliegors.dev/")) failures.push(`${route}: invalid canonical`);
  if (!attribute(description, "content")) failures.push(`${route}: missing description`);
  if (!attribute(robots, "content")) failures.push(`${route}: missing robots policy`);
  if (route !== "/404.html" && !jsonLd) failures.push(`${route}: missing JSON-LD`);
  if (route !== "/404.html" && !["en", "es", "x-default"].every((language) => alternates.some((node) => attribute(node, "hreflang") === language))) {
    failures.push(`${route}: incomplete language alternates`);
  }
  if (/^\/(?:es\/)?docs\/.+/.test(route)) {
    const sections = elements(document, "section").filter((node) => (attribute(node, "class") ?? "").split(/\s+/).includes("rs-doc-section"));
    if (sections.length < 4) failures.push(`${route}: expected at least four documented contract sections`);
  }
  if (jsonLd) {
    try {
      const schema = JSON.parse((jsonLd.childNodes ?? []).map((node) => node.value ?? "").join(""));
      const graph = schema["@graph"] ?? [];
      if (/^\/(?:es\/)?docs\/.+/.test(route)) {
        if (graph[0]?.["@type"] !== "TechArticle") failures.push(`${route}: documentation schema is not TechArticle`);
        if (graph[1]?.itemListElement?.length !== 3) failures.push(`${route}: documentation breadcrumb is not hierarchical`);
      }
      if (route === "/" || route === "/es") {
        if (!graph.some((node) => node["@type"] === "SoftwareSourceCode")) failures.push(`${route}: missing SoftwareSourceCode schema`);
      }
    } catch {
      failures.push(`${route}: invalid JSON-LD`);
    }
  }
}

for (const asset of [
  "assets/pliegors.css",
  "assets/pliegors_site_boot.js",
  "assets/pliegors_site_client.js",
  "assets/pliegors_site_client_bg.wasm",
  "favicon.svg",
  "robots.txt",
  "site.webmanifest",
  "sitemap-index.xml",
  "sitemap-0.xml",
]) {
  try {
    if ((await stat(path.join(root, asset))).size === 0) failures.push(`${asset}: empty`);
  } catch {
    failures.push(`${asset}: missing`);
  }
}

const homeHtml = await readFile(path.join(root, "index.html"), "utf8").catch(() => "");
const siteCss = await readFile(path.join(root, "assets/pliegors.css"), "utf8").catch(() => "");
const homeDocument = parse(homeHtml);
const mobileMenu = elements(homeDocument, "div").find((node) => attribute(node, "data-mobile-menu") === "");
if (
  !mobileMenu
  || attribute(mobileMenu, "aria-hidden") !== "true"
  || attribute(mobileMenu, "role") !== "dialog"
  || attribute(mobileMenu, "aria-modal") !== "true"
) {
  failures.push("mobile menu: missing closed accessibility state");
}
if (!siteCss.includes("html.menu-is-open body")) {
  failures.push("mobile menu: missing document scroll lock contract");
}
if (!siteCss.includes(".rs-doc-card[hidden]") || !siteCss.includes("display: none")) {
  failures.push("docs search: hidden result contract is missing");
}
if (!siteCss.includes("[data-reveal].is-reveal-pending")) {
  failures.push("progressive enhancement: reveal pending state is not explicitly client-owned");
}
if (!/\[data-reveal\]\s*\{[^}]*opacity:\s*1;[^}]*transform:\s*none;/s.test(siteCss)) {
  failures.push("progressive enhancement: reveal content is not visible before client admission");
}
if (!siteCss.includes("::-webkit-search-cancel-button")) {
  failures.push("docs search: native clear affordance is not suppressed");
}
for (const hook of ["data-engine-lab", "data-pipeline", "data-hero-carousel"]) {
  if (!homeHtml.includes(`${hook}=\"\"`)) failures.push(`home: missing ${hook} interaction contract`);
}
for (const asset of ["/media/pliegors/fold-hero.webp", "/media/pliegors/ledger-wide.webp"]) {
  if (!homeHtml.includes(asset)) failures.push(`home: missing authored brand asset ${asset}`);
}
for (const statement of ["R0-R7", "accepted private candidate"]) {
  if (!homeHtml.toLowerCase().includes(statement.toLowerCase())) failures.push(`home: missing current candidate statement ${statement}`);
}

const docsHtml = await readFile(path.join(root, "docs/index.html"), "utf8").catch(() => "");
const docsDocument = parse(docsHtml);
const docsItems = elements(docsDocument, "a").filter((node) => attribute(node, "data-docs-item") === "");
if (docsItems.length !== 18) failures.push(`docs index: expected 18 topics, found ${docsItems.length}`);

const cliSource = await readFile(path.join(repository, "crates/pliego-cli/src/main.rs"), "utf8");
const cliGuide = await readFile(path.join(root, "docs/cli/index.html"), "utf8").catch(() => "");
for (const command of [
  "pliego new <path>",
  "pliego templates",
  "pliego check",
  "pliego build",
  "pliego dev",
  "pliego preview",
  "pliego inspect",
  "pliego why artifact <path|route>",
  "pliego why-rebuilt",
  "pliego version",
]) {
  if (!cliSource.includes(command)) failures.push(`CLI source: expected command ${command}`);
  if (!cliGuide.includes(command.replaceAll("<", "&lt;").replaceAll(">", "&gt;"))) {
    failures.push(`CLI guide: missing command ${command}`);
  }
}

const crateGuide = await readFile(path.join(root, "docs/crate-reference/index.html"), "utf8").catch(() => "");
for (const crate of [
  "pliego-dom",
  "pliego-macros",
  "pliego-log",
  "pliego-fold",
  "pliego-reactive",
  "pliego-content",
  "pliego-artifact",
  "pliego-ssg",
  "pliego-resume",
  "pliego-adapters",
  "pliego-assets",
  "pliego-inspect",
  "pliego-hyphae",
  "pliego-starters",
  "pliego-cli",
]) {
  if (!crateGuide.includes(crate)) failures.push(`crate reference: missing ${crate}`);
}

const sitemap = await readFile(path.join(root, "sitemap-0.xml"), "utf8").catch(() => "");
for (const route of expected.filter((route) => route !== "/404.html")) {
  const canonical = route === "/" ? "/" : `${route.replace(/\/$/, "")}/`;
  if (!sitemap.includes(`<loc>https://pliegors.dev${canonical}</loc>`)) failures.push(`${route}: missing sitemap entry`);
}
if ((sitemap.match(/<url>/g) ?? []).length !== expected.length - 1) failures.push("sitemap: unexpected URL count");

const walk = async (directory) => {
  const output = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const current = path.join(directory, entry.name);
    if (entry.isDirectory()) output.push(...await walk(current));
    else output.push(current);
  }
  return output;
};

const sourceRoots = [
  path.join(repository, "docs"),
  path.join(repository, "examples", "pliegors-site"),
  path.join(repository, "examples", "pliegors-site-client"),
  path.join(repository, "workers", "pliegors-site"),
];
const ignoredSourceDirectories = new Set([".git", ".wrangler", "node_modules", "target"]);
const walkSource = async (directory) => {
  const output = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.isDirectory() && ignoredSourceDirectories.has(entry.name)) continue;
    const current = path.join(directory, entry.name);
    if (entry.isDirectory()) output.push(...await walkSource(current));
    else output.push(current);
  }
  return output;
};
const localPathPattern = /(?:[A-Za-z]:[\\/](?:Users|Documents)[\\/]|\/Users\/[^/\s]+\/|\/home\/[^/\s]+\/)/;
for (const sourceRoot of sourceRoots) {
  for (const file of await walkSource(sourceRoot)) {
    if (!/\.(?:md|rs|toml|jsonc?|mjs|ts|css)$/i.test(file)) continue;
    const source = await readFile(file, "utf8");
    if (localPathPattern.test(source)) {
      failures.push(`${path.relative(repository, file)}: machine-local absolute path`);
    }
  }
}
for (const file of await walk(root)) {
  const relative = path.relative(root, file).replaceAll("\\", "/").toLowerCase();
  for (const term of forbidden) {
    if (relative.includes(term.replace(/^\//, ""))) failures.push(`${relative}: forbidden path trace ${term}`);
  }
  if (!/\.(?:html|css|js|json|xml|svg|txt|webmanifest)$/i.test(file)) continue;
  const source = (await readFile(file, "utf8")).toLowerCase();
  for (const term of forbidden) {
    if (source.includes(term)) failures.push(`${path.relative(root, file)}: forbidden trace ${term}`);
  }
}

const htmlFiles = (await walk(root)).filter((file) => file.toLowerCase().endsWith(".html"));
const htmlByPath = new Map();
for (const file of htmlFiles) {
  const relative = path.relative(root, file).replaceAll("\\", "/");
  htmlByPath.set(`/${relative}`, parse(await readFile(file, "utf8")));
}
const targetDocument = (pathname) => {
  if (pathname === "/") return "/index.html";
  if (pathname.endsWith("/")) return `${pathname}index.html`;
  if (path.posix.extname(pathname)) return pathname;
  return `${pathname}/index.html`;
};
for (const [sourcePath, document] of htmlByPath) {
  for (const anchor of elements(document, "a")) {
    const href = attribute(anchor, "href");
    if (!href || /^(?:https?:|mailto:|tel:)/.test(href)) continue;
    const sourceRoute = sourcePath === "/index.html"
      ? "/"
      : sourcePath.replace(/index\.html$/, "");
    const resolved = new URL(href, `https://pliegors.dev${sourceRoute}`);
    const documentPath = targetDocument(resolved.pathname);
    const destination = htmlByPath.get(documentPath);
    if (!destination) {
      failures.push(`${sourcePath}: broken local link ${href}`);
      continue;
    }
    if (resolved.hash) {
      const fragment = decodeURIComponent(resolved.hash.slice(1));
      const hasTarget = elements(destination, "*").some((node) => attribute(node, "id") === fragment);
      if (!hasTarget) failures.push(`${sourcePath}: missing fragment target ${href}`);
    }
  }
}

for (const stale of ["cases/index.html", "es/cases/index.html"]) {
  try {
    await stat(path.join(root, stale));
    failures.push(`${stale}: stale public output remains`);
  } catch {
    // Expected absence.
  }
}

if (failures.length) {
  console.error(failures.map((failure) => `- ${failure}`).join("\n"));
  process.exit(1);
}
console.log(`PliegoRS site contract passed: ${expected.length} routes, canonical SEO, bilingual alternates, private examples absent.`);
