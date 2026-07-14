import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFile,
  mkdir,
  readdir,
  readFile,
  stat,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outputRoot = path.join(repoRoot, "fixtures", "manifests");

const argumentsMap = parseArguments(process.argv.slice(2));
const syncProjects = argumentsMap.get("sync-projects") === "true";
const specifications = [
  {
    key: "site",
    work: {
      kind: "house",
      id: "pliegors-site",
      slug: "pliegors-site",
      title: "PliegoRS Site",
      owner: "Celiums Solutions LLC / PliegoRS",
    },
    trackedExtensions: [
      ".avif",
      ".ico",
      ".jpg",
      ".mp4",
      ".pdf",
      ".png",
      ".svg",
      ".webp",
      ".woff2",
      ".zip",
    ],
    output: "pliegors-site.asset-manifest.json",
  },
];

await mkdir(outputRoot, { recursive: true });
for (const specification of specifications) {
  const projectRoot = argumentsMap.get(specification.key);
  if (!projectRoot) {
    throw new Error(`Missing --${specification.key} <project-root>`);
  }
  const publicRoot = path.join(path.resolve(projectRoot), "public");
  const manifest = await buildManifest(specification, publicRoot);
  const outputPath = path.join(outputRoot, specification.output);
  const serialized = `${JSON.stringify(manifest, null, 2)}\n`;
  await writeFile(outputPath, serialized, "utf8");
  if (syncProjects) {
    await writeFile(
      path.join(path.resolve(projectRoot), "pliego.asset-manifest.json"),
      serialized,
      "utf8",
    );
  }
  process.stdout.write(
    `${specification.work.id}: ${manifest.assets.length} assets -> ${outputPath}\n`,
  );
}

if (syncProjects) {
  const siteRoot = path.resolve(argumentsMap.get("site"));
  const schemaTargets = [path.join(siteRoot, "public", "schemas")];
  const schemaFiles = [
    "pliego.budget-waivers.schema.json",
    "pliego.asset-manifest.schema.json",
    "pliego.device-fingerprint.schema.json",
    "pliego.measurement-plan.schema.json",
    "pliego.measurement-report.schema.json",
    "pliego.measurement-run.schema.json",
  ];
  for (const schemaTarget of schemaTargets) {
    await mkdir(schemaTarget, { recursive: true });
    for (const fileName of schemaFiles) {
      await copyFile(path.join(repoRoot, "schemas", fileName), path.join(schemaTarget, fileName));
    }
  }
}

function parseArguments(values) {
  const parsed = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const option = values[index];
    const value = values[index + 1];
    if (!option?.startsWith("--") || !value) {
      throw new Error("Expected a --site <project-root> path pair");
    }
    parsed.set(option.slice(2), value);
  }
  return parsed;
}

async function buildManifest(specification, publicRoot) {
  const files = (await walk(publicRoot))
    .map((filePath) => ({
      absolute: filePath,
      relative: toPosix(path.relative(publicRoot, filePath)),
    }))
    .filter(({ absolute }) =>
      specification.trackedExtensions.includes(path.extname(absolute).toLowerCase()),
    )
    .sort((left, right) => left.relative.localeCompare(right.relative, "en"));

  const variants = [];
  const assets = [];
  for (const file of files) {
    const metadata = await describeFile(file.absolute);
    const classification = classify(specification.key, file.relative);
    const variantId = identifier(`v-${file.relative}`);
    const variant = {
      id: variantId,
      path: file.relative,
      mediaType: mediaType(file.relative),
      format: identifier(path.extname(file.relative).slice(1).toLowerCase()),
      sha256: await sha256(file.absolute),
      bytes: (await stat(file.absolute)).size,
      tiers: classification.tiers,
      delivery: classification.delivery,
      preload: classification.preload,
    };
    if (metadata.width && metadata.height) {
      variant.dimensions = {
        width: metadata.width,
        height: metadata.height,
      };
      variant.estimatedVramBytes = metadata.width * metadata.height * 4;
    }
    if (metadata.durationMs) {
      variant.durationMs = metadata.durationMs;
    }
    const fallbackTarget = fallbackTargetFor(file.relative, files);
    if (fallbackTarget) {
      variant.fallbackFor = [identifier(`v-${fallbackTarget}`)];
    }

    variants.push(variant);
    assets.push({
      id: identifier(`asset-${file.relative}`),
      label: labelFromPath(file.relative),
      owner: classification.owner,
      source: {
        kind: classification.sourceKind,
        locator: `public/${file.relative}`,
        ...(classification.generator
          ? { generator: classification.generator }
          : {}),
        createdFor: specification.work.id,
      },
      rights: classification.rights,
      visualImportance: classification.importance,
      tags: classification.tags,
      variants: [variant],
    });
  }

  return {
    $schema: "https://pliegors.dev/schemas/pliego.asset-manifest.schema.json",
    manifestVersion: "1.0.0",
    work: specification.work,
    coverage: {
      assetRoot: "public",
      trackedExtensions: specification.trackedExtensions,
      excluded: [],
    },
    assets,
    budgetScopes: buildBudgets(specification.key, variants),
  };
}

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries.sort((left, right) =>
    left.name.localeCompare(right.name, "en"),
  )) {
    const child = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await walk(child)));
    } else if (entry.isFile()) {
      files.push(child);
    }
  }
  return files;
}

function classify(project, relativePath) {
  const lower = relativePath.toLowerCase();
  const extension = path.extname(lower);
  const isVideo = extension === ".mp4";
  const isDownload = extension === ".zip" || extension === ".pdf";
  const isFont = extension === ".woff2";
  const isCritical =
    isFont ||
    lower.includes("hero") ||
    lower.includes("favicon") ||
    lower.includes("mark.svg") ||
    lower.endsWith("archive-hero.jpg");
  const tiers = isVideo
    ? ["balanced", "signature"]
    : ["universal", "lite", "balanced", "signature"];
  let delivery = "on-demand";
  if (isDownload) delivery = "download";
  else if (isVideo) delivery = "deferred";
  else if (isCritical) delivery = "initial";

  if (isFont) {
    return {
      owner: { entity: "Respective typeface authors", role: "third-party" },
      sourceKind: "licensed",
      generator: undefined,
      rights: {
        status: "licensed",
        holder: "Respective typeface authors",
        transfer: "excluded",
        license: "SIL Open Font License 1.1",
      },
      importance: "critical",
      tiers,
      delivery,
      preload: lower.includes("instrument-sans-variable.woff2"),
      tags: ["font", "third-party"],
    };
  }

  const generatedMedia = [".avif", ".jpg", ".mp4", ".png", ".webp"].includes(
    extension,
  );
  const tag = isVideo ? "video" : isDownload ? "delivery" : "visual";
  return {
    owner: { entity: "Celiums Solutions LLC / PliegoRS", role: "house" },
    sourceKind: generatedMedia ? "generated" : "owned",
    generator: generatedMedia ? "PliegoRS asset pipeline" : undefined,
    rights: {
      status: "owned",
      holder: "Celiums Solutions LLC",
      transfer: "excluded",
    },
    importance: isCritical ? "critical" : isVideo ? "supporting" : "utility",
    tiers,
    delivery,
    preload: false,
    tags: [tag, "framework-surface"],
  };
}

function buildBudgets(project, variants) {
  const byPath = new Map(variants.map((variant) => [variant.path, variant]));
  if (project !== "site") throw new Error(`Unsupported project: ${project}`);
  const firstHero = byPath.get("media/pliegors/fold-hero.webp");
  const initialPaths = [
    firstHero?.path,
    "fonts/fragment-mono-regular.woff2",
    "fonts/instrument-sans-variable.woff2",
    "favicon.svg",
  ].filter(Boolean);
  return [
    budget(
      "home-universal-initial",
      "PliegoRS home first viewport",
      "/",
      "universal",
      "initial",
      byPath,
      initialPaths,
      { maxTransferBytes: 358400 },
    ),
  ];
}

function budget(id, label, route, tier, phase, byPath, paths, limits) {
  const missing = paths.filter((assetPath) => !byPath.has(assetPath));
  if (missing.length) {
    throw new Error(`Budget ${id} references missing paths: ${missing.join(", ")}`);
  }
  return {
    id,
    label,
    route,
    tier,
    phase,
    variants: paths.map((assetPath) => byPath.get(assetPath).id),
    limits,
  };
}

async function describeFile(filePath) {
  const extension = path.extname(filePath).toLowerCase();
  if (![".avif", ".jpg", ".mp4", ".png", ".webp"].includes(extension)) {
    return {};
  }
  try {
    const output = execFileSync(
      "ffprobe",
      [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,duration:format=duration",
        "-of",
        "json",
        filePath,
      ],
      { encoding: "utf8", windowsHide: true },
    );
    const result = JSON.parse(output);
    const stream = result.streams?.[0] ?? {};
    const duration = Number(stream.duration ?? result.format?.duration ?? 0);
    return {
      width: Number(stream.width) || undefined,
      height: Number(stream.height) || undefined,
      durationMs: duration > 0 ? Math.round(duration * 1000) : undefined,
    };
  } catch {
    return {};
  }
}

async function sha256(filePath) {
  return createHash("sha256").update(await readFile(filePath)).digest("hex");
}

function fallbackTargetFor(relativePath, files) {
  if (path.extname(relativePath).toLowerCase() !== ".webp") return undefined;
  const candidate = `${withoutExtension(relativePath)}.avif`;
  return files.some((file) => file.relative === candidate) ? candidate : undefined;
}

function mediaType(relativePath) {
  const extension = path.extname(relativePath).toLowerCase();
  return new Map([
    [".avif", "image/avif"],
    [".ico", "image/x-icon"],
    [".jpg", "image/jpeg"],
    [".mp4", "video/mp4"],
    [".pdf", "application/pdf"],
    [".png", "image/png"],
    [".svg", "image/svg+xml"],
    [".webp", "image/webp"],
    [".woff2", "font/woff2"],
    [".zip", "application/zip"],
  ]).get(extension);
}

function withoutExtension(relativePath) {
  return relativePath.slice(0, -path.extname(relativePath).length);
}

function identifier(value) {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function labelFromPath(relativePath) {
  const base = path.basename(withoutExtension(relativePath));
  return base
    .split(/[-_]+/g)
    .filter(Boolean)
    .map((word) => `${word[0].toUpperCase()}${word.slice(1)}`)
    .join(" ");
}

function toPosix(value) {
  return value.split(path.sep).join("/");
}
