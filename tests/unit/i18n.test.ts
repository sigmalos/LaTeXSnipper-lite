import { afterEach, describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { setLang, t, translateDom, translationKeys } from "../../frontend/lib/i18n";

afterEach(() => setLang("zh"));

describe("i18n", () => {
  it("returns Chinese source text by default and interpolates", () => {
    expect(t("清空")).toBe("清空");
    expect(t("共 {count} 页", { count: 3 })).toBe("共 3 页");
    expect(t("共 {count} 页")).toBe("共 {count} 页");
  });

  it("translates to English and falls back to the key", () => {
    setLang("en");
    expect(t("截图识别")).toBe("Capture");
    expect(t("正在识别第 {page}/{total} 页", { page: 1, total: 2 })).toBe("Recognizing page 1 of 2");
    expect(t("未翻译")).toBe("未翻译");
  });

  it("translates tagged DOM nodes", () => {
    setLang("en");
    document.body.innerHTML = `<b data-i18n="设置">设置</b><input data-i18n-placeholder="名称"><i data-i18n-title="复制"></i>`;
    translateDom(document);
    expect(document.querySelector("b")!.textContent).toBe("Settings");
    expect(document.querySelector("input")!.placeholder).toBe("Name");
    expect(document.querySelector("i")!.title).toBe("Copy");
  });

  it("every data-i18n key in index.html and t() call in main.ts has an English entry", () => {
    const html = readFileSync("index.html", "utf8");
    const main = readFileSync("frontend/main.ts", "utf8");
    const used = new Set<string>();
    for (const m of html.matchAll(/data-i18n(?:-placeholder|-title)?="([^"]+)"/g)) used.add(m[1]);
    for (const m of main.matchAll(/\bt\(\s*"([^"]+)"/g)) used.add(m[1]);
    for (const m of main.matchAll(/\["([^"]+)", "([^"]+)"\]/g)) used.add(m[1]), used.add(m[2]);
    for (const m of main.matchAll(/\b(?:title|body): "([^"]+)"/g)) used.add(m[1]);
    const missing = [...used].filter((k) => /[\u4e00-\u9fff]/.test(k) && !translationKeys.includes(k));
    expect(missing).toEqual([]);
  });
});
