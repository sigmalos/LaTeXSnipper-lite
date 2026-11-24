#!/usr/bin/env node
// Write latest.json, the manifest the in-app updater reads from the latest
// GitHub release, from the signed update packages in a directory.
//   node scripts/update-manifest.mjs <dir> <version> <base-url>
// <dir> holds the installers and their .sig files; <base-url> is where the
// release serves them (…/releases/download/v1.2.3). The notes are the
// "{{VERSION}} 更新内容" section of .github/release-notes.md.
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// Updater targets ({os}-{arch}[-{installer}]) and the package each one installs.
const TARGETS = [
  { keys: ["darwin-aarch64", "darwin-aarch64-app"], match: /_macos-arm64\.app\.tar\.gz$/ },
  { keys: ["windows-x86_64", "windows-x86_64-nsis"], match: /_windows-x64-setup\.exe$/ },
  // Only the .deb: an AppImage updater must not pick it up.
  { keys: ["linux-x86_64-deb"], match: /_linux-amd64\.deb$/ },
];

/** The release notes' own section, with the placeholders filled in. */
export function releaseNotes(template, version, officeVersion) {
  const filled = template.replaceAll("{{VERSION}}", version).replaceAll("{{OFFICE_VERSION}}", officeVersion);
  const start = filled.indexOf(`## ${version} 更新内容`);
  if (start < 0) return "";
  const body = filled.slice(filled.indexOf("\n", start) + 1);
  const end = body.search(/^## /m);
  return (end < 0 ? body : body.slice(0, end)).trim();
}

/** `files` maps package names to their signature (contents of the .sig file). */
export function buildManifest({ version, notes, pubDate, baseUrl, files }) {
  const platforms = {};
  for (const { keys, match } of TARGETS) {
    const name = Object.keys(files).find((f) => match.test(f));
    if (!name) continue;
    const entry = { signature: files[name].trim(), url: `${baseUrl.replace(/\/$/, "")}/${encodeURIComponent(name)}` };
    for (const key of keys) platforms[key] = entry;
  }
  if (!Object.keys(platforms).length) throw new Error("no signed update packages found");
  return { version, notes, pub_date: pubDate, platforms };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [dir, version, baseUrl] = process.argv.slice(2);
  if (!dir || !version || !baseUrl) {
    console.error("usage: update-manifest.mjs <dir> <version> <base-url>");
    process.exit(2);
  }
  const root = new URL("..", import.meta.url);
  const template = readFileSync(new URL(".github/release-notes.md", root), "utf8");
  const office = JSON.parse(readFileSync(new URL("office_plugin/package.json", root), "utf8")).version;
  const files = {};
  for (const f of readdirSync(dir)) {
    if (f.endsWith(".sig")) files[f.slice(0, -4)] = readFileSync(join(dir, f), "utf8");
  }
  const manifest = buildManifest({
    version,
    notes: releaseNotes(template, version, office),
    pubDate: new Date().toISOString(),
    baseUrl,
    files,
  });
  writeFileSync(join(dir, "latest.json"), JSON.stringify(manifest, null, 2) + "\n");
  console.log(`latest.json: ${version} for ${Object.keys(manifest.platforms).join(", ")}`);
}
