// Build LaTeXSnipper Office.app and its DMG, separately from the desktop app.
//
//   cd office_plugin && npm ci && npm run build
//
// Output: office_plugin/release/LaTeXSnipperOffice_<version>_macos_<arch>.dmg
//
// Steps: assemble the served files, build the host (latexsnipper-office-host)
// as the app's sidecar, then let Tauri bundle the Setup app around both and
// sign it ad hoc. Nothing from the desktop app's build is used.

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { assemble } from "./assemble.mjs";

const plugin = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const app = join(plugin, "app");
const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { stdio: "inherit", cwd: plugin, ...opts });
const read = (cmd, args) => execFileSync(cmd, args, { cwd: plugin, encoding: "utf8" });

if (process.platform !== "darwin") throw new Error("LaTeXSnipper Office for Mac must be built on macOS.");

// One version for the app, the host and the add-in manifest.
const version = JSON.parse(readFileSync(join(app, "tauri.conf.json"), "utf8")).version;
const cargoVersion = (file) => readFileSync(file, "utf8").match(/^version = "([^"]+)"/m)[1];
const versions = {
  "app/tauri.conf.json": version,
  "app/Cargo.toml": cargoVersion(join(app, "Cargo.toml")),
  "host/Cargo.toml": cargoVersion(join(plugin, "host/Cargo.toml")),
  "package.json": JSON.parse(readFileSync(join(plugin, "package.json"), "utf8")).version,
};
if (new Set(Object.values(versions)).size !== 1) throw new Error(`version mismatch: ${JSON.stringify(versions)}`);

console.log("› assembling add-in files");
assemble();

console.log("› building the host");
run("cargo", ["build", "--release", "--manifest-path", "host/Cargo.toml", "--bin", "latexsnipper-office-host"]);
const triple = read("rustc", ["-vV"]).match(/^host: (.+)$/m)[1];
mkdirSync(join(app, "binaries"), { recursive: true });
copyFileSync(join(plugin, "host/target/release/latexsnipper-office-host"), join(app, "binaries", `latexsnipper-office-host-${triple}`));

console.log("› bundling LaTeXSnipper Office.app");
// Explicit paths: the repository also contains the desktop app's Tauri project.
run(join(plugin, "node_modules/.bin/tauri"), ["build", "--bundles", "app,dmg"], {
  env: { ...process.env, TAURI_APP_PATH: app, TAURI_FRONTEND_PATH: plugin },
});

const bundle = join(app, "target/release/bundle");
const appPath = join(bundle, "macos/LaTeXSnipper Office.app");
for (const part of ["Contents/MacOS/latexsnipper-office-host", "Contents/Resources/office_addin/web/taskpane.html", "Contents/Resources/office_addin/MathJax/startup.js"]) {
  if (!existsSync(join(appPath, part))) throw new Error(`bundle is missing ${part}`);
}
run("codesign", ["--verify", "--deep", "--strict", appPath]);
// The bundled host serves the bundled files (temporary certificate and port; nothing installed).
run(join(appPath, "Contents/MacOS/latexsnipper-office-host"), ["--self-test"]);

const dmgDir = join(bundle, "dmg");
const built = readdirSync(dmgDir).filter((f) => f.endsWith(".dmg")).map((f) => join(dmgDir, f)).sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
const arch = triple.startsWith("aarch64") ? "arm64" : triple.split("-")[0];
mkdirSync(join(plugin, "release"), { recursive: true });
const output = join(plugin, "release", `LaTeXSnipperOffice_${version}_macos_${arch}.dmg`);
copyFileSync(built, output);
console.log(`\n${output} (${(statSync(output).size / 1e6).toFixed(1)} MB)`);
