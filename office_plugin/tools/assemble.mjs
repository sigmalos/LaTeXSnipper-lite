// Assemble the files the add-in host serves to Word and PowerPoint:
//
//   build/office_addin/
//     web/                      add-in pages (office_plugin/web)
//     shared/                   shared editor (office_plugin/shared/EditorAssets)
//     support/                  help-page images
//     MathJaxTypographyAdapter.js
//     manifest.xml              template; the host fills in {{BASE_URL}} and {{VERSION}}
//     MathJax/                  the MathJax 4.1.3 "office" resource profile
//
// The MathJax profile is the one the original LaTeXSnipper ships: upstream
// files come from the exact npm versions in package.json, and every file is
// checked against the SHA-256 recorded in mathjax/resources.json.
//
//   node tools/assemble.mjs [output directory]

import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const plugin = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = resolve(process.argv[2] ?? join(plugin, "build/office_addin"));

// Files written by LaTeXSnipper rather than taken from npm.
const AUTHORED = new Set(["config.js", "runtime.js", "office.js", "resources.json"]);
// npm package of each fonts/<name>/ directory in the profile.
const FONT_PACKAGES = {
  "mathjax-tex-font": "@mathjax/mathjax-tex-font",
  "mathjax-stix2-font": "@mathjax/mathjax-stix2-font",
  "mathjax-mhchem-font-extension": "@mathjax/mathjax-mhchem-font-extension",
};

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const skipJunk = { filter: (src) => !src.endsWith(".DS_Store") };

export function sourceOf(relative) {
  if (AUTHORED.has(relative)) return join(plugin, "mathjax", relative);
  const font = relative.match(/^fonts\/([^/]+)\/(.+)$/);
  if (font) {
    const pkg = FONT_PACKAGES[font[1]];
    if (!pkg) throw new Error(`no npm package for ${relative}`);
    return join(plugin, "node_modules", pkg, font[2]);
  }
  return join(plugin, "node_modules/mathjax", relative);
}

export function assemble(target = output) {
  rmSync(target, { recursive: true, force: true });
  mkdirSync(target, { recursive: true });
  cpSync(join(plugin, "web"), join(target, "web"), { recursive: true, ...skipJunk });
  cpSync(join(plugin, "shared/EditorAssets"), join(target, "shared"), { recursive: true, ...skipJunk });
  cpSync(join(plugin, "support"), join(target, "support"), { recursive: true, ...skipJunk });
  cpSync(join(plugin, "shared/MathJaxTypographyAdapter.js"), join(target, "MathJaxTypographyAdapter.js"));
  cpSync(join(plugin, "manifest.xml"), join(target, "manifest.xml"));

  const resources = JSON.parse(readFileSync(join(plugin, "mathjax/resources.json"), "utf8"));
  if (resources.version !== "4.1.3") throw new Error(`unexpected MathJax version ${resources.version}`);
  const profile = resources.profiles.office;
  const problems = [];
  for (const relative of profile) {
    const source = sourceOf(relative);
    if (!existsSync(source)) {
      problems.push(`missing ${relative} (${source})`);
      continue;
    }
    const bytes = readFileSync(source);
    // Authored files are tracked in git here; upstream files must match their recorded hash.
    const expected = resources.files[relative]?.sha256;
    if (!AUTHORED.has(relative) && (!expected || expected !== sha256(bytes))) {
      problems.push(`checksum mismatch: ${relative}`);
      continue;
    }
    const destination = join(target, "MathJax", relative);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, bytes);
  }
  if (problems.length) {
    throw new Error(`MathJax office profile is incomplete. Run npm ci in office_plugin.\n  ${problems.join("\n  ")}`);
  }
  return { target, files: profile.length };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { target, files } = assemble();
  console.log(`assembled ${target} (MathJax: ${files} files verified)`);
}
