import { describe, expect, it } from "vitest";
// @ts-expect-error -- plain Node script without type declarations
import { buildManifest, releaseNotes } from "../../scripts/update-manifest.mjs";

const TEMPLATE = `> [!IMPORTANT]
> 提示

## {{VERSION}} 更新内容

### 改进与修复

- 修复 A，插件仍为 {{OFFICE_VERSION}}

## 下载

| 系统 | 文件 |
`;

describe("update manifest", () => {
  it("takes only the version's own section of the release notes", () => {
    expect(releaseNotes(TEMPLATE, "1.4.0", "1.1.0")).toBe("### 改进与修复\n\n- 修复 A，插件仍为 1.1.0");
    expect(releaseNotes("no sections", "1.4.0", "1.1.0")).toBe("");
  });

  it("maps each signed package to its updater targets", () => {
    const m = buildManifest({
      version: "1.4.0",
      notes: "n",
      pubDate: "2026-11-24T00:00:00.000Z",
      baseUrl: "https://github.com/o/r/releases/download/v1.4.0/",
      files: {
        "LaTeXSnipper-Lite_1.4.0_macos-arm64.app.tar.gz": "sigM\n",
        "LaTeXSnipper-Lite_1.4.0_windows-x64-setup.exe": "sigW",
        "LaTeXSnipper-Lite_1.4.0_linux-amd64.deb": "sigL",
      },
    });
    const base = "https://github.com/o/r/releases/download/v1.4.0";
    expect(m.platforms["darwin-aarch64"]).toEqual({ signature: "sigM", url: `${base}/LaTeXSnipper-Lite_1.4.0_macos-arm64.app.tar.gz` });
    expect(m.platforms["darwin-aarch64-app"]).toEqual(m.platforms["darwin-aarch64"]);
    expect(m.platforms["windows-x86_64-nsis"].url).toBe(`${base}/LaTeXSnipper-Lite_1.4.0_windows-x64-setup.exe`);
    expect(m.platforms["linux-x86_64-deb"]).toEqual({ signature: "sigL", url: `${base}/LaTeXSnipper-Lite_1.4.0_linux-amd64.deb` });
    expect(m.platforms["linux-x86_64"]).toBeUndefined();
    expect(m).toMatchObject({ version: "1.4.0", notes: "n", pub_date: "2026-11-24T00:00:00.000Z" });
  });

  it("refuses a release without signed packages", () => {
    expect(() => buildManifest({ version: "1", notes: "", pubDate: "", baseUrl: "x", files: {} })).toThrow(/no signed/);
  });
});
