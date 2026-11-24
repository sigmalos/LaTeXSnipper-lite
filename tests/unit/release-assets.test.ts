import { describe, expect, it } from "vitest";
// @ts-expect-error -- plain Node script without type declarations
import { checkAssets, releaseAssets } from "../../scripts/release-assets.mjs";
// @ts-expect-error -- plain Node script without type declarations
import { buildManifest } from "../../scripts/update-manifest.mjs";

const ALL = [
  "LaTeXSnipper-Lite_1.5.0_macos-arm64.dmg",
  "LaTeXSnipper-Lite_1.5.0_windows-x64-setup.exe",
  "LaTeXSnipper-Lite_1.5.0_linux-amd64.deb",
  "latexsnipper-office_1.1.0_macos-arm64.dmg",
  "LaTeXSnipper-Lite_1.5.0_macos-arm64.app.tar.gz",
  "latest.json",
];

describe("release files", () => {
  it("names the four installers and the two update files, Office in lowercase", () => {
    expect(Object.values(releaseAssets("1.5.0", "1.1.0"))).toEqual(ALL);
  });

  it("accepts exactly the release files, in any order", () => {
    expect(checkAssets([...ALL].reverse(), "1.5.0", "1.1.0")).toEqual([]);
  });

  it("rejects anything extra: signatures, checksums, old names", () => {
    const problems = checkAssets(
      [...ALL, "SHA256SUMS.txt", "LaTeXSnipper-Lite_1.5.0_linux-amd64.deb.sig", "latexsnipper-lite_1.5.0_amd64.deb"],
      "1.5.0",
      "1.1.0",
    );
    expect(problems).toEqual([
      "not allowed in a release: SHA256SUMS.txt",
      "not allowed in a release: LaTeXSnipper-Lite_1.5.0_linux-amd64.deb.sig",
      "not allowed in a release: latexsnipper-lite_1.5.0_amd64.deb",
    ]);
  });

  it("reports missing files and the wrong version", () => {
    expect(checkAssets(ALL.slice(1), "1.5.0", "1.1.0")).toEqual(["missing: LaTeXSnipper-Lite_1.5.0_macos-arm64.dmg"]);
    // Four app files carry the version; the Office DMG and latest.json do not.
    expect(checkAssets(ALL, "1.5.1", "1.1.0")).toHaveLength(8);
    expect(checkAssets(["LaTeXSnipper-Office_1.1.0_macos-arm64.dmg", ...ALL.filter((n) => !n.startsWith("latexsnipper-office"))], "1.5.0", "1.1.0")).toEqual([
      "missing: latexsnipper-office_1.1.0_macos-arm64.dmg",
      "not allowed in a release: LaTeXSnipper-Office_1.1.0_macos-arm64.dmg",
    ]);
  });

  it("latest.json points only at files the release carries", () => {
    const a = releaseAssets("1.5.0", "1.1.0");
    const m = buildManifest({
      version: "1.5.0",
      notes: "",
      pubDate: "",
      baseUrl: "https://example.com/v1.5.0",
      files: { [a.macosUpdate]: "s", [a.windows]: "s", [a.linux]: "s" },
    });
    const urls = new Set(Object.values(m.platforms).map((p: unknown) => decodeURIComponent((p as { url: string }).url.split("/").pop()!)));
    for (const name of urls) expect(Object.values(a)).toContain(name);
    expect(urls.size).toBe(3);
  });
});
