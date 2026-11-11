#!/usr/bin/env node
// Print the app version after checking package.json, Cargo.toml and
// tauri.conf.json agree. With a tag argument (v1.2.3) it must match too.
//   node scripts/check-version.mjs [v1.2.3]
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const versions = {
  "package.json": JSON.parse(read("package.json")).version,
  "package-lock.json": JSON.parse(read("package-lock.json")).version,
  "backend/Cargo.toml": /^version\s*=\s*"([^"]+)"/m.exec(read("backend/Cargo.toml"))?.[1],
  "backend/tauri.conf.json": JSON.parse(read("backend/tauri.conf.json")).version,
};
const tag = process.argv[2];
if (tag) versions[`tag ${tag}`] = tag.replace(/^refs\/tags\//, "").replace(/^v/, "");

const distinct = [...new Set(Object.values(versions))];
if (distinct.length !== 1 || !/^\d+\.\d+\.\d+$/.test(distinct[0] ?? "")) {
  console.error("Version mismatch:");
  for (const [file, v] of Object.entries(versions)) console.error(`  ${file.padEnd(26)} ${v ?? "(missing)"}`);
  process.exit(1);
}
console.log(distinct[0]);
