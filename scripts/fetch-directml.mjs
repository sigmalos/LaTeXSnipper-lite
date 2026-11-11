#!/usr/bin/env node
// Put the DirectML redistributable next to the Windows build so ONNX Runtime
// can use the GPU. Windows ships an older DirectML.dll in System32, and ONNX
// Runtime 1.22 needs feature level 5.0 (DirectML 1.10 or newer), so without
// this file the app quietly falls back to the CPU.
//
// Only bin/x64-win/DirectML.dll is read out of the 200 MB NuGet package, using
// HTTP range requests; the result is checked against a pinned SHA-256.
//   node scripts/fetch-directml.mjs            # writes backend/resources/windows/DirectML.dll
//   DIRECTML_NUPKG_URL=<mirror> node scripts/fetch-directml.mjs
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateRawSync } from "node:zlib";

const VERSION = "1.15.4";
const ENTRY = "bin/x64-win/DirectML.dll";
const SHA256 = "9C9E6D822561C6C41B90E6994B3E8857CF1D66DBFB1E0C4C799C7C89B4E92DA1";
const SOURCES = [
  process.env.DIRECTML_NUPKG_URL,
  `https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/${VERSION}/microsoft.ai.directml.${VERSION}.nupkg`,
  `https://www.nuget.org/api/v2/package/Microsoft.AI.DirectML/${VERSION}`,
].filter(Boolean);
const out = fileURLToPath(new URL("../backend/resources/windows/DirectML.dll", import.meta.url));

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex").toUpperCase();

async function range(url, start, end) {
  const res = await fetch(url, {
    headers: { Range: end === undefined ? `bytes=${start}` : `bytes=${start}-${end}` },
    signal: AbortSignal.timeout(120_000),
  });
  if (res.status !== 206) throw new Error(`${url}: expected 206 Partial Content, got ${res.status}`);
  const total = Number(res.headers.get("content-range")?.split("/")[1]);
  return { buf: Buffer.from(await res.arrayBuffer()), total };
}

// Read one entry of a remote zip without downloading the whole archive.
async function extract(url, name) {
  const { buf: tail, total } = await range(url, -65536);
  const eocd = tail.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (eocd < 0) throw new Error("not a zip archive");
  const cdSize = tail.readUInt32LE(eocd + 12);
  const cdOffset = tail.readUInt32LE(eocd + 16);
  const cd =
    cdOffset >= total - tail.length
      ? tail.subarray(cdOffset - (total - tail.length), cdOffset - (total - tail.length) + cdSize)
      : (await range(url, cdOffset, cdOffset + cdSize - 1)).buf;
  for (let p = 0; p + 46 <= cd.length && cd.readUInt32LE(p) === 0x02014b50; ) {
    const method = cd.readUInt16LE(p + 10);
    const csize = cd.readUInt32LE(p + 20);
    const nlen = cd.readUInt16LE(p + 28);
    const skip = nlen + cd.readUInt16LE(p + 30) + cd.readUInt16LE(p + 32);
    const local = cd.readUInt32LE(p + 42);
    const entry = cd.toString("utf8", p + 46, p + 46 + nlen);
    p += 46 + skip;
    if (entry !== name) continue;
    const { buf: header } = await range(url, local, local + 29);
    const start = local + 30 + header.readUInt16LE(26) + header.readUInt16LE(28);
    const { buf: data } = await range(url, start, start + csize - 1);
    if (method === 0) return data;
    if (method === 8) return inflateRawSync(data);
    throw new Error(`unsupported zip compression method ${method}`);
  }
  throw new Error(`${name} not found in the package`);
}

if (existsSync(out) && sha256(readFileSync(out)) === SHA256) {
  console.log(`DirectML ${VERSION} already present: ${out}`);
  process.exit(0);
}

const errors = [];
for (const url of SOURCES) {
  try {
    const dll = await extract(url, ENTRY);
    const hash = sha256(dll);
    if (hash !== SHA256) throw new Error(`checksum mismatch: got ${hash}`);
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, dll);
    console.log(`DirectML ${VERSION} (${(dll.length / 1e6).toFixed(1)} MB) -> ${out}`);
    process.exit(0);
  } catch (e) {
    errors.push(`  ${url}\n    ${e.message}`);
  }
}
console.error(`Could not fetch DirectML ${VERSION}:\n${errors.join("\n")}`);
console.error("Set DIRECTML_NUPKG_URL to a NuGet mirror of Microsoft.AI.DirectML and retry.");
process.exit(1);
