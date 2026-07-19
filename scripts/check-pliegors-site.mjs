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
  "/docs/opensdk",
  "/docs/opensdk-components",
  "/docs/browser-framework-conformance",
  "/docs/opensdk-tooling",
  "/docs/opensdk-compatibility",
  "/docs/assets",
  "/docs/artifact-trust",
  "/docs/release-trust",
  "/docs/performance-evidence",
  "/docs/errors-and-diagnostics",
  "/docs/telemetry",
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
      if (route === "/security" || route === "/es/security") {
        if (graph[0]?.mainEntity?.["@type"] !== "ContactPoint") failures.push(`${route}: missing security ContactPoint schema`);
        if (graph[0]?.mainEntity?.email !== "hello@pliegors.dev") failures.push(`${route}: security ContactPoint email drift`);
        if (graph[0]?.about?.length !== 3) failures.push(`${route}: incomplete security topic schema`);
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
  ".well-known/security.txt",
  "site.webmanifest",
  "sitemap-index.xml",
  "sitemap-0.xml",
  "media/pliegors/security-trust.avif",
  "media/pliegors/security-trust.webp",
  "fonts/LICENSE-fragment-mono.txt",
  "fonts/LICENSE-instrument-sans.txt",
  "fonts/LICENSE-instrument-serif.txt",
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
for (const statement of ["R0-R7", "public preview"]) {
  if (!homeHtml.toLowerCase().includes(statement.toLowerCase())) failures.push(`home: missing current release statement ${statement}`);
}
if (!homeHtml.includes("PLIEGORS / 0.0.2 / PUBLIC PREVIEW")) {
  failures.push("home: current release marker is not 0.0.2");
}

const changelogPages = [
  ["/changelog", path.join(root, "changelog/index.html"), "OpenSDK becomes executable."],
  ["/es/changelog", path.join(root, "es/changelog/index.html"), "OpenSDK se vuelve ejecutable."],
];
for (const [route, file, localizedTitle] of changelogPages) {
  const html = await readFile(file, "utf8").catch(() => "");
  const document = parse(html);
  const entries = elements(document, "article")
    .map((node) => attribute(node, "data-change-entry"))
    .filter(Boolean);
  if (JSON.stringify(entries) !== JSON.stringify(["unreleased", "v0-0-2", "v0-0-1"])) {
    failures.push(`${route}: changelog entries are incomplete or out of order`);
  }
  for (const required of [
    localizedTitle,
    "0.0.2",
    "2026-07-18",
    "https://github.com/celiumsai/pliegors/releases/tag/v0.0.2",
    "https://github.com/celiumsai/pliegors/blob/main/CHANGELOG.md",
  ]) {
    if (!html.includes(required)) failures.push(`${route}: missing current changelog contract ${required}`);
  }
}

const docsHtml = await readFile(path.join(root, "docs/index.html"), "utf8").catch(() => "");
const docsDocument = parse(docsHtml);
const docsItems = elements(docsDocument, "a").filter((node) => attribute(node, "data-docs-item") === "");
if (docsItems.length !== 26) failures.push(`docs index: expected 26 topics, found ${docsItems.length}`);
for (const required of [
  "RELEASE / 0.0.2 + OPENSDK / PREVIEW",
  "pliego-sdk is not on crates.io",
  "/docs/opensdk",
]) {
  if (!docsHtml.includes(required)) failures.push(`docs index: missing release boundary ${required}`);
}

const cliSource = await readFile(path.join(repository, "crates/pliego-cli/src/main.rs"), "utf8");
const cliGuide = await readFile(path.join(root, "docs/cli/index.html"), "utf8").catch(() => "");
for (const command of [
  "pliego new <path>",
  "pliego templates",
  "pliego doctor",
  "pliego report --bundle",
  "pliego upgrade --check",
  "pliego telemetry <status|enable|preview|export|disable>",
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
  "pliego-sdk",
]) {
  if (!crateGuide.includes(crate)) failures.push(`crate reference: missing ${crate}`);
}

const opensdkPages = [
  ["/docs/opensdk", ["0.1.0-preview.1", "pliego-sdk is not published on crates.io", "Rust 1.86.0", "RFC-006", "ADR-006"]],
  ["/docs/opensdk-components", ["pliego:build/transformer@0.1.0", "TypeScript", "Python", "npm run check:opensdk:multilang", "native-trusted"]],
  ["/docs/browser-framework-conformance", ["React", "Svelte", "Lit", "npm run check:opensdk:browser-frameworks", "MessageChannel"]],
  ["/docs/opensdk-tooling", ["JSON-RPC 2.0", "MCP 2025-11-25", "pliego/handshake", "10,000", "1 MiB"]],
  ["/docs/opensdk-compatibility", ["pliego.sdk-compatibility-matrix.schema.json", "experimental", "preview", "stable", "provider-neutral"]],
];
for (const [route, requiredTerms] of opensdkPages) {
  const html = await readFile(outputPath(route), "utf8").catch(() => "");
  for (const required of requiredTerms) {
    if (!html.includes(required)) failures.push(`${route}: missing OpenSDK contract ${required}`);
  }
  const spanishRoute = `/es${route}`;
  const spanishHtml = await readFile(outputPath(spanishRoute), "utf8").catch(() => "");
  if (!spanishHtml.includes("0.1.0-preview.1")) {
    failures.push(`${spanishRoute}: missing localized OpenSDK version boundary`);
  }
}

const securityPages = [
  ["/security", path.join(root, "security/index.html"), "en"],
  ["/es/security", path.join(root, "es/security/index.html"), "es"],
];
for (const [route, file, language] of securityPages) {
  const html = await readFile(file, "utf8").catch(() => "");
  const document = parse(html);
  const boundaries = elements(document, "li").filter((node) => attribute(node, "data-security-boundary"));
  const evidence = elements(document, "tr").filter((node) => attribute(node, "data-security-evidence"));
  const limitations = elements(document, "article").filter((node) => attribute(node, "data-security-limitation"));
  if (boundaries.length !== 5) failures.push(`${route}: expected five explicit trust boundaries, found ${boundaries.length}`);
  if (evidence.length < 6) failures.push(`${route}: expected at least six evidence rows, found ${evidence.length}`);
  if (limitations.length !== 5) failures.push(`${route}: expected five honest claim limitations, found ${limitations.length}`);
  for (const required of [
    "19",
    "R0–R7",
    "Ed25519",
    "pliegors-candidate-2026-01",
    "node verify-release-bundle.mjs",
    "--dir .",
    "sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250",
    "/.well-known/security.txt",
    "hello@pliegors.dev",
    "RUSTSEC-2026-0173",
  ]) {
    if (!html.includes(required)) failures.push(`${route}: missing security contract ${required}`);
  }
  const languageClaims = language === "es"
    ? ["3 días hábiles", "7 días hábiles", "Investigación de buena fe", "Sin advisories publicados"]
    : ["3 business days", "7 business days", "Good-faith research", "No published advisories"];
  for (const claim of languageClaims) {
    if (!html.includes(claim)) failures.push(`${route}: missing disclosure claim ${claim}`);
  }
  if (!html.includes("/media/pliegors/security-trust.avif") || !html.includes("/media/pliegors/security-trust.webp")) {
    failures.push(`${route}: missing authored security trust media`);
  }
  if (!html.includes("github.com/celiumsai/pliegors/blob/main/SECURITY.md")) {
    failures.push(`${route}: missing canonical repository security policy`);
  }
}

const securityTxt = await readFile(path.join(root, ".well-known/security.txt"), "utf8").catch(() => "");
const securityTxtLines = securityTxt.trim().split(/\r?\n/);
for (const field of [
  "Contact: mailto:hello@pliegors.dev",
  "Preferred-Languages: en, es",
  "Canonical: https://pliegors.dev/.well-known/security.txt",
  "Policy: https://pliegors.dev/security/",
]) {
  if (!securityTxtLines.includes(field)) failures.push(`security.txt: missing ${field}`);
}
const expiryLine = securityTxtLines.find((line) => line.startsWith("Expires: "));
const expiry = Date.parse(expiryLine?.slice("Expires: ".length) ?? "");
if (!Number.isFinite(expiry) || expiry <= Date.now() + (30 * 24 * 60 * 60 * 1000)) {
  failures.push("security.txt: expiry must remain at least 30 days in the future");
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
      const assetPath = path.join(root, resolved.pathname.replace(/^\//, ""));
      try {
        if ((await stat(assetPath)).size === 0) failures.push(`${sourcePath}: empty local asset ${href}`);
      } catch {
        failures.push(`${sourcePath}: broken local link ${href}`);
      }
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
console.log(`PliegoRS site contract passed: ${expected.length} routes, canonical SEO, bilingual alternates, product examples absent.`);
