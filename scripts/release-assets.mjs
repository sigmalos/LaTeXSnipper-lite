#!/usr/bin/env node
// The files a GitHub release may carry, and a check that it carries exactly those.
//
//   node scripts/release-assets.mjs list <version> <office-version>
//   node scripts/release-assets.mjs check-dir <dir> <version> <office-version>
//   node scripts/release-assets.mjs check-release <tag> <version> <office-version>
//
// Installers: the macOS app and the Office add-in DMGs, the Windows setup.exe
// and the Linux .deb. The in-app updater also needs latest.json and the macOS
// update package (.app.tar.gz; Windows and Linux update from the installers).
// Signatures live inside latest.json; nothing else is published.
import { execFileSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

/** Every file a release of `version` (Office add-in `office`) must carry, by role. */
export function releaseAssets(version, office) {
  return {
    macos: `LaTeXSnipper-Lite_${version}_macos-arm64.dmg`,
    windows: `LaTeXSnipper-Lite_${version}_windows-x64-setup.exe`,
    linux: `LaTeXSnipper-Lite_${version}_linux-amd64.deb`,
    office: `latexsnipper-office_${office}_macos-arm64.dmg`,
    macosUpdate: `LaTeXSnipper-Lite_${version}_macos-arm64.app.tar.gz`,
    manifest: "latest.json",
  };
}

/** Problems with a set of file names: anything missing, and anything extra. */
export function checkAssets(names, version, office) {
  const expected = Object.values(releaseAssets(version, office));
  const missing = expected.filter((n) => !names.includes(n));
  const extra = names.filter((n) => !expected.includes(n));
  return [
    ...missing.map((n) => `missing: ${n}`),
    ...extra.map((n) => `not allowed in a release: ${n}`),
  ];
}

function report(where, problems) {
  if (problems.length) {
    console.error(`${where} does not hold exactly the release files:`);
    for (const p of problems) console.error(`  ${p}`);
    process.exit(1);
  }
  console.log(`${where}: exactly the ${Object.keys(releaseAssets("", "")).length} release files`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [cmd, target, ...rest] = process.argv.slice(2);
  if (cmd === "list") {
    for (const name of Object.values(releaseAssets(target, rest[0]))) console.log(name);
  } else if (cmd === "check-dir" && rest.length === 2) {
    report(target, checkAssets(readdirSync(target), ...rest));
  } else if (cmd === "check-release" && rest.length === 2) {
    const json = execFileSync("gh", ["release", "view", target, "--json", "assets"], { encoding: "utf8" });
    report(`release ${target}`, checkAssets(JSON.parse(json).assets.map((a) => a.name), ...rest));
  } else {
    console.error("usage: release-assets.mjs list <version> <office-version>\n" +
      "       release-assets.mjs check-dir <dir> <version> <office-version>\n" +
      "       release-assets.mjs check-release <tag> <version> <office-version>");
    process.exit(2);
  }
}
