import { expect, test } from "@playwright/test";
import { existsSync } from "node:fs";

const WORD_XSL = "/Applications/Microsoft Word.app/Contents/Resources/mathml2omml.xsl";
const TYPOGRAPHY = {
  typographyVersion: 1,
  symbolFontId: "mathjax-tex",
  numberFontFamily: "Times New Roman",
  cjkFontFamily: "PingFang SC",
  defaultMathStyle: "Automatic",
  fontSizePoints: 14,
  color: "#336699",
};

// office.js is Microsoft's CDN script; these pages run without Office here.
test.beforeEach(async ({ page }) => {
  await page.route("https://appsforoffice.microsoft.com/**", (route) => route.abort());
});

test("the help page and its images load", async ({ page }) => {
  await page.goto("/addin/help.html");
  await expect(page.locator("h2").first()).toBeVisible();
  const broken = await page.$$eval("img", (imgs) => imgs.filter((i) => !i.complete || i.naturalWidth === 0).map((i) => i.src));
  expect(broken).toEqual([]);
  await expect(page.getByText("不需要打开 LaTeXSnipper 桌面端")).toBeVisible();
});

test("renders SVG with MathJax 4, the typography adapter and CoreText outlines", async ({ page }) => {
  await page.goto("/addin/help.html");
  const outlineCalls = [];
  page.on("request", (r) => r.url().endsWith("/api/font-outlines") && outlineCalls.push(r.url()));
  const result = await page.evaluate(async (typography) => {
    const renderer = await import("/addin/app/renderer.mjs");
    const out = await renderer.renderSvg("x^{2}+\\frac{12}{y}=\\text{面积}", true, typography);
    const doc = new DOMParser().parseFromString(out.svg, "image/svg+xml");
    return {
      root: doc.documentElement.localName,
      paths: doc.getElementsByTagName("path").length,
      width: out.widthPoints,
      height: out.heightPoints,
      color: out.svg.toLowerCase().includes("#336699"),
      warnings: out.warnings,
    };
  }, TYPOGRAPHY);
  expect(result.root).toBe("svg");
  expect(result.paths).toBeGreaterThan(3);
  expect(result.width).toBeGreaterThan(20);
  expect(result.height).toBeGreaterThan(10);
  expect(result.color).toBe(true);
  // Digits (Times New Roman) and CJK text are drawn from system font outlines.
  expect(outlineCalls.length).toBeGreaterThan(0);
});

test("converts to Word OMML through the host", async ({ page }) => {
  test.skip(!existsSync(WORD_XSL), "Microsoft Word is not installed");
  await page.goto("/addin/help.html");
  const omml = await page.evaluate(async (typography) => {
    const renderer = await import("/addin/app/renderer.mjs");
    const { api } = await import("/addin/app/api.mjs");
    const mathml = await renderer.renderMathMl("\\sum_{k=1}^{n} k^{2}", true, typography);
    return (await api("/api/omml", { method: "POST", body: { mathml, typography } })).omml;
  }, TYPOGRAPHY);
  expect(omml).toContain("<m:nary>");
  expect(omml).toContain('w:val="336699"');
  expect(omml).toContain('w:val="28"');
});

test("the pages see the host's catalog and settings", async ({ page }) => {
  await page.goto("/addin/help.html");
  const out = await page.evaluate(async () => {
    const { catalog, loadSettings } = await import("/addin/app/api.mjs");
    const c = await catalog();
    const s = await loadSettings("powerpoint");
    return { version: c.mathjaxVersion, fonts: c.symbolFonts, backend: s.insertionBackend };
  });
  expect(out).toEqual({ version: "4.1.3", fonts: ["mathjax-tex", "mathjax-stix2"], backend: "Png" });
});
