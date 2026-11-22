import { expect, test } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

test("PDF pages are rendered, recognized in mixed mode and inserted", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__MOCK_SEED__ = { outcomes: [{ text: "Page one $a^2$" }, { text: "Page two $x$" }] };
  });
  await page.addInitScript({ path: path.join(here, "tauri-mock.js") });
  await page.goto("/");
  await expect(page.locator("body[data-ready=true]")).toBeAttached();

  await page.setInputFiles("#pdf-input", path.join(here, "two-pages.pdf"));
  await expect(page.locator("#pdf-dialog")).toBeVisible();
  await expect(page.locator("#pdf-pages")).toHaveText("共 2 页");

  await page.fill("#pdf-range", "3");
  await page.click("#pdf-start");
  await expect(page.locator(".toast.error")).toContainText("页码范围无效");

  await page.fill("#pdf-range", "1-2");
  await page.fill("#pdf-dpi", "100");
  await page.click("#pdf-start");
  await expect(page.locator("#pdf-status")).toHaveText("PDF 识别结果", { timeout: 15000 });
  await expect(page.locator("#pdf-text")).toHaveValue("<!-- page 1 -->\n\nPage one $a^2$\n\n<!-- page 2 -->\n\nPage two $x$");

  const calls = await page.evaluate(() => (window as any).__mock.calls.filter((c: any) => c.cmd === "recognize_image"));
  expect(calls).toHaveLength(2);
  expect(calls.every((c: any) => c.args.mode === "mixed" && c.args.data.startsWith("data:image/png;base64,"))).toBe(true);

  await page.click("#pdf-insert");
  await expect(page.locator("#editor")).toHaveValue(/Page two \$x\$/);
  await expect(page.locator("#preview p")).toHaveCount(4);
});

test("PDF recognition uses whichever formula detector is installed", async ({ page }) => {
  // The standard detector is selected (the default), but only the Int8 one is on disk.
  await page.addInitScript(() => {
    (window as any).__MOCK_SEED__ = {
      installedModels: ["mathcraft-formula-det-int8", "mathcraft-formula-rec", "mathcraft-text-det", "mathcraft-text-rec"],
      outcomes: [{ text: "Page one $a^2$" }],
    };
  });
  await page.addInitScript({ path: path.join(here, "tauri-mock.js") });
  await page.goto("/");
  await expect(page.locator("body[data-ready=true]")).toBeAttached();

  await page.setInputFiles("#pdf-input", path.join(here, "two-pages.pdf"));
  await page.fill("#pdf-range", "1");
  await page.click("#pdf-start");
  await expect(page.locator("#pdf-status")).toHaveText("PDF 识别结果", { timeout: 15000 });
  await expect(page.locator("#models-dialog")).toBeHidden();
  const cmds = await page.evaluate(() => (window as any).__mock.calls.map((c: any) => c.cmd));
  expect(cmds).not.toContain("download_models");

  // In Settings, the Int8 detector is the one in use, so deleting it warns that mixed mode breaks.
  await page.keyboard.press("Escape");
  await page.click("#settings-btn");
  await page.locator('.model-row[data-model="mathcraft-formula-det-int8"] .model-delete').click();
  await expect(page.locator("#ask-text")).toHaveText("删除“公式检测(Int8)”模型？混合识别将无法使用，需要时可重新下载（20 MB）。");
});
