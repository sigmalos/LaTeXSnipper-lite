import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(here, "../backend/tests/fixtures/frac.png");

type Seed = Record<string, unknown>;

async function open(page: Page, seed: Seed = {}): Promise<void> {
  await page.addInitScript((s) => ((window as unknown as { __MOCK_SEED__: Seed }).__MOCK_SEED__ = s), seed);
  await page.addInitScript({ path: path.join(here, "tauri-mock.js") });
  await page.goto("/");
  await expect(page.locator("body[data-ready=true]")).toBeAttached();
}

const mock = <T>(page: Page, expr: string) => page.evaluate((e) => new Function(`return window.__mock.${e}`)(), expr) as Promise<T>;
const historyRows = (page: Page) => page.locator("#history li:not(.empty)");

test.describe("main window", () => {
  test("renders the original layout and status", async ({ page }) => {
    await open(page);
    for (const label of ["截图识别", "历史记录", "LaTeX 编辑器", "图片识别", "PDF 识别", "手写识别", "复制", "导出", "实时渲染预览"]) {
      await expect(page.getByText(label, { exact: true }).first()).toBeVisible();
    }
    await expect(page.locator("#status")).toContainText("当前模型: MathCraft 公式 | 状态: 就绪");
    await expect(page.locator("#capture-kbd")).toHaveText("⌘F");
    await expect(page.locator("#history .empty")).toContainText("⌘F");
  });

  test("live preview renders LaTeX and reports TeX errors", async ({ page }) => {
    await open(page);
    await page.fill("#editor", "\\frac{a}{b}");
    await expect(page.locator("#preview mjx-container svg")).toBeVisible();
    await page.fill("#editor", "\\frac{a}{");
    await expect(page.locator("#preview-error")).toBeVisible();
    await page.fill("#editor", "Mixed $x^2$ text");
    await expect(page.locator("#preview p")).toContainText("Mixed");
    await expect(page.locator("#preview p mjx-container")).toHaveCount(1);
    await page.click("#clear-preview-btn");
    await expect(page.locator("#editor")).toHaveValue("");
    await expect(page.locator("#preview mjx-container")).toHaveCount(0);
  });
});

test.describe("recognition", () => {
  test("capture → review → confirm copies and records history", async ({ page }) => {
    await open(page, { outcomes: [{ text: "\\sqrt{x}" }] });
    await page.click("#capture-btn");
    const dlg = page.locator("#result-dialog");
    await expect(dlg).toBeVisible();
    await expect(page.locator("#result-text")).toHaveValue("\\sqrt{x}");
    await expect(page.locator("#result-preview mjx-container")).toBeVisible();
    await page.fill("#result-text", "\\sqrt{y}");
    await page.click("#result-confirm");
    await expect(dlg).toBeHidden();
    await expect(page.locator(".toast")).toContainText("已确认并复制到剪贴板");
    await expect.poll(() => mock<string>(page, "clipboard")).toBe("\\sqrt{y}");
    await expect(historyRows(page)).toHaveCount(1);
    await expect(historyRows(page).first().locator(".src")).toHaveText("\\sqrt{y}");
    await expect(page.locator("#editor")).toHaveValue("\\sqrt{y}");
  });

  test("cancelled capture does nothing; cancel in review keeps history empty", async ({ page }) => {
    await open(page, { outcomes: [null, { text: "a" }] });
    await page.click("#capture-btn");
    await expect(page.locator("#result-dialog")).toBeHidden();
    await page.click("#capture-btn");
    await page.locator("#result-dialog").getByText("取消").click();
    await expect(historyRows(page)).toHaveCount(0);
  });

  test("global hotkey event triggers capture", async ({ page }) => {
    await open(page, { outcomes: [{ text: "e^{x}" }] });
    await page.evaluate(() => (window as any).__mock.emit("hotkey-capture", null));
    await expect(page.locator("#result-text")).toHaveValue("e^{x}");
  });

  test("low confidence results are flagged", async ({ page }) => {
    await open(page, { outcomes: [{ text: "{a", score: 0.4, flags: ["unbalanced_group"] }] });
    await page.click("#capture-btn");
    await expect(page.locator("#result-warn")).toContainText("unbalanced_group");
  });

  test("image file upload sends a data URL to the backend", async ({ page }) => {
    await open(page, { outcomes: [{ text: "\\frac{a+b}{c^{2}}" }] });
    await page.setInputFiles("#image-input", FIXTURE);
    await expect(page.locator("#result-text")).toHaveValue("\\frac{a+b}{c^{2}}");
    const calls = await mock<{ cmd: string; args: { data: string; mode: string } }[]>(page, "calls");
    const call = calls.find((c) => c.cmd === "recognize_image")!;
    expect(call.args.data.startsWith("data:image/png;base64,")).toBe(true);
    expect(call.args.mode).toBe("formula");
  });

  test("empty and failing recognitions show actionable messages", async ({ page }) => {
    await open(page, { outcomes: [{ error: "EMPTY_RESULT" }, { error: "decoder crashed" }] });
    await page.click("#capture-btn");
    await expect(page.locator(".toast.error")).toContainText("未检测到可识别内容");
    await page.click("#capture-btn");
    await expect(page.locator(".toast.error").last()).toContainText("识别失败：decoder crashed");
  });

  test("missing models open the download sheet and progress to ready", async ({ page }) => {
    await open(page, { models: { formula: false, mixed: false, text: false } });
    await expect(page.locator("#status")).toContainText("需下载模型");
    await page.click("#capture-btn");
    await expect(page.locator("#models-dialog")).toBeVisible();
    await expect(page.locator("#models-text")).toContainText("112 MB");
    await page.click("#models-start");
    await expect(page.locator("#models-dialog")).toBeHidden();
    await expect(page.locator("#status")).toContainText("就绪");
  });

  test("download failures are explained and retryable", async ({ page }) => {
    await open(page, { models: { formula: false, mixed: false, text: false }, downloadFails: true });
    await page.click("#capture-btn");
    await page.click("#models-start");
    await expect(page.locator("#models-status")).toContainText("下载失败：network unreachable");
    await expect(page.locator("#models-start")).toBeEnabled();
  });
});

test.describe("history and favorites", () => {
  const seeded = {
    history: [
      { text: "a+b", name: "", kind: "mathcraft" },
      { text: "c^2", name: "勾股", kind: "mathcraft" },
    ],
  };

  test("orders newest first and toggles", async ({ page }) => {
    await open(page, seeded);
    await expect(historyRows(page).first().locator(".num")).toHaveText("#2");
    await expect(historyRows(page).first().locator(".name")).toHaveText("勾股");
    await page.click("#order-btn");
    await expect(historyRows(page).first().locator(".num")).toHaveText("#1");
    await expect(page.locator("#order-label")).toHaveText("最早在前");
    await expect.poll(() => mock<boolean>(page, "settings.history_reverse")).toBe(false);
  });

  test("click loads into editor; copy button copies", async ({ page }) => {
    await open(page, seeded);
    await historyRows(page).nth(1).locator(".src").click();
    await expect(page.locator("#editor")).toHaveValue("a+b");
    await historyRows(page).first().hover();
    await historyRows(page).first().getByLabel("复制到剪贴板").click();
    await expect.poll(() => mock<string>(page, "clipboard")).toBe("c^2");
  });

  test("rename, favorite and delete via the item menu", async ({ page }) => {
    await open(page, seeded);
    const row = historyRows(page).nth(1); // "a+b"
    await row.click({ button: "right" });
    await page.locator("#item-menu").getByText("重命名").click();
    await page.fill("#ask-input", "加法");
    await page.click("#ask-ok");
    await expect(historyRows(page).nth(1).locator(".name")).toHaveText("加法");

    await historyRows(page).nth(1).click({ button: "right" });
    await page.locator("#item-menu").getByText("收藏").click();
    await expect(page.locator(".toast")).toContainText("已收藏");

    await historyRows(page).first().click({ button: "right" });
    await page.locator("#item-menu").getByText("删除").click();
    await expect(historyRows(page)).toHaveCount(1);

    await page.click("#favorites-btn");
    const favs = page.locator("#favorites li:not(.empty)");
    await expect(favs).toHaveCount(1);
    await expect(favs.first().locator(".name")).toHaveText("加法");
    await favs.first().locator(".src").click();
    await expect(page.locator("#favorites-dialog")).toBeHidden();
    await expect(page.locator("#editor")).toHaveValue("a+b");
  });

  test("clearing history asks for confirmation", async ({ page }) => {
    await open(page, seeded);
    await page.click("#clear-history-btn");
    await expect(page.locator("#ask-text")).toContainText("2 条历史记录");
    await page.locator("#ask-dialog").getByText("取消").click();
    await expect(historyRows(page)).toHaveCount(2);
    await page.click("#clear-history-btn");
    await page.click("#ask-ok");
    await expect(historyRows(page)).toHaveCount(0);
  });

  test("favorites export writes Markdown", async ({ page }) => {
    await open(page, { favorites: [{ text: "e^{i\\pi}+1=0", name: "Euler", kind: "mathcraft" }], savePath: "/tmp/fav.md" });
    await page.click("#favorites-btn");
    await page.click("#favorites-export");
    await expect(page.locator(".toast")).toContainText("/tmp/fav.md");
    const exported = await mock<{ content: string; format: string }[]>(page, "exported");
    expect(exported[0].format).toBe("md");
    expect(exported[0].content).toContain("## Euler");
    expect(exported[0].content).toContain("$$\ne^{i\\pi}+1=0\n$$");
  });
});

test.describe("copy and export", () => {
  test("export menu produces format-specific payloads", async ({ page }) => {
    await open(page, { savePath: "/tmp/f.tex" });
    await page.fill("#editor", "x^2");
    await page.click("#export-btn");
    await page.locator("#export-menu").getByText("LaTeX (.tex)").click();
    await page.click("#export-btn");
    await page.locator("#export-menu").getByText("MathML (.mml)").click();
    await page.click("#export-btn");
    await page.locator("#export-menu").getByText("SVG (.svg)").click();
    await expect.poll(() => mock<unknown[]>(page, "exported").then((e) => e.length)).toBe(3);
    const [tex, mml, svg] = await mock<{ content: string }[]>(page, "exported");
    expect(tex.content).toContain("\\begin{document}\n\\[\nx^2\n\\]");
    expect(mml.content).toContain("<math");
    expect(svg.content).toMatch(/^<svg[^>]*xmlns="http:\/\/www.w3.org\/2000\/svg"/);
  });

  test("Word export is disabled without Pandoc", async ({ page }) => {
    await open(page);
    await page.click("#export-btn");
    await expect(page.locator("#export-menu").getByText(/Word/)).toBeDisabled();
  });

  test("copy-as wraps LaTeX", async ({ page }) => {
    await open(page);
    await page.fill("#editor", "a=b");
    await page.click("#export-btn");
    await page.locator("#export-menu").getByText("equation").click();
    await expect.poll(() => mock<string>(page, "clipboard")).toBe("\\begin{equation}\na=b\n\\end{equation}");
    await page.click("#copy-btn");
    await expect.poll(() => mock<string>(page, "clipboard")).toBe("a=b");
  });

  test("empty editor exports nothing", async ({ page }) => {
    await open(page);
    await page.click("#copy-btn");
    await expect(page.locator(".toast.error")).toContainText("内容为空");
  });
});

test.describe("settings", () => {
  test("switching mode, theme and language persists and re-renders", async ({ page }) => {
    await open(page);
    await page.click("#settings-btn");
    await page.locator("#settings-form").getByText("混合", { exact: true }).click();
    await page.locator("#settings-form").getByText("深色", { exact: true }).click();
    await page.selectOption("select[name=language]", "en");
    await page.locator("#settings-form").getByText("保存", { exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(page.locator("#capture-btn")).toContainText("Capture");
    await expect(page.locator("#status")).toContainText("Model: MathCraft Mixed | Status: Ready");
    const s = await mock<{ mode: string; theme: string; language: string }>(page, "settings");
    expect([s.mode, s.theme, s.language]).toEqual(["mixed", "dark", "en"]);
  });

  test("theme options apply on click, before saving", async ({ page }) => {
    await open(page); // saved theme: light
    await page.click("#settings-btn");
    const themes = page.locator("#settings-form .field");
    await expect(themes.getByText("跟随系统", { exact: true })).toBeVisible();
    await expect(themes.locator("input[value=light]")).toBeChecked();

    await themes.getByText("深色", { exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect.poll(() => mock<string | null>(page, "windowTheme")).toBe("dark");
    await expect(page.locator("#settings-dialog")).toBeVisible(); // still previewing

    await themes.getByText("浅色", { exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect.poll(() => mock<string | null>(page, "windowTheme")).toBe("light");
    expect(await mock<string>(page, "settings.theme")).toBe("light"); // nothing saved yet
  });

  test("follow system previews the system appearance and tracks changes", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark" });
    await open(page); // saved theme: light
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.click("#settings-btn");
    await page.locator("#settings-form .field").getByText("跟随系统", { exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect.poll(() => mock<string | null>(page, "windowTheme")).toBeNull();

    // A system appearance change during the preview must not snap back to the saved theme.
    await page.emulateMedia({ colorScheme: "light" });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

    await page.locator("#settings-form").getByText("保存", { exact: true }).click();
    await expect.poll(() => mock<string>(page, "settings.theme")).toBe("auto");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  });

  test("cancel and Esc restore the saved theme", async ({ page }) => {
    await open(page); // saved theme: light
    const themes = page.locator("#settings-form .field");
    await page.click("#settings-btn");
    await themes.getByText("深色", { exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page.locator("#settings-form").getByText("取消", { exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect.poll(() => mock<string | null>(page, "windowTheme")).toBe("light");

    await page.click("#settings-btn");
    await themes.getByText("深色", { exact: true }).click();
    await page.keyboard.press("Escape");
    await expect(page.locator("#settings-dialog")).toBeHidden();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    expect(await mock<string>(page, "settings.theme")).toBe("light");

    // Reopening shows the saved choice, not the abandoned preview.
    await page.click("#settings-btn");
    await expect(themes.locator("input[value=light]")).toBeChecked();
  });

  test("theme can be chosen from the keyboard", async ({ page }) => {
    await open(page);
    await page.click("#settings-btn");
    await page.locator("#settings-form input[name=theme][value=light]").focus();
    await page.keyboard.press("ArrowRight");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  });

  test("built-in backend lists all four models with their download state", async ({ page }) => {
    await open(page, { models: { formula: true, mixed: false, text: false } });
    await page.click("#settings-btn");
    const panel = page.locator("#builtin-fields");
    await expect(panel).toBeVisible();
    await expect(page.locator("#models-count")).toHaveText("已下载 1/4");
    const rows = panel.locator(".model-row");
    await expect(rows).toHaveCount(4);
    await expect(rows.locator(".model-name")).toHaveText(["公式检测", "公式识别", "文字检测", "文字识别"]);
    await expect(rows.locator(".model-use")).toHaveText(["用于：混合", "用于：公式、混合", "用于：混合、纯文字", "用于：混合、纯文字"]);
    await expect(rows.locator(".model-size")).toHaveText(["64 MB", "104 MB", "4.3 MB", "13 MB"]);
    await expect(rows.locator(".model-state")).toHaveText(["未下载", "已下载", "未下载", "未下载"]);
    await expect(rows.locator(".model-download")).toHaveText(["下载", "重新下载", "下载", "下载"]);
  });

  test("the model list follows the selected backend before saving", async ({ page }) => {
    await open(page);
    await page.click("#settings-btn");
    const form = page.locator("#settings-form");
    await form.getByText("外部模型（OpenAI 兼容接口）").click();
    await expect(page.locator("#builtin-fields")).toBeHidden();
    await expect(page.locator("#external-fields")).toBeVisible();
    await form.getByText("内置 MathCraft（离线）").click();
    await expect(page.locator("#builtin-fields")).toBeVisible();
    await expect(page.locator("#external-fields")).toBeHidden();
  });

  test("the model list is hidden when an external model is configured", async ({ page }) => {
    await open(page, { settings: { backend: "external", external_model: "qwen2.5vl:7b" } });
    await page.click("#settings-btn");
    await expect(page.locator("#builtin-fields")).toBeHidden();
    await expect(page.locator("#external-fields")).toBeVisible();
  });

  test("downloads individual models on demand", async ({ page }) => {
    await open(page, { models: { formula: true, mixed: false, text: false }, settings: { mode: "text" }, downloadDelay: 150 });
    await expect(page.locator("#status")).toContainText("需下载模型");
    await page.click("#settings-btn");
    const row = (id: string) => page.locator(`.model-row[data-model="${id}"]`);

    await row("mathcraft-text-det").locator(".model-download").click();
    await expect(row("mathcraft-text-det")).toHaveClass(/is-downloading/);
    await expect(row("mathcraft-text-det").locator(".model-download")).toBeDisabled();
    await expect(row("mathcraft-text-det").locator(".model-state")).toHaveText(/下载中 \d+%/);
    await expect(row("mathcraft-text-det").locator(".model-state")).toHaveText("已下载");
    await expect(row("mathcraft-text-det").locator(".model-download")).toHaveText("重新下载");
    await expect(page.locator("#models-count")).toHaveText("已下载 2/4");
    await expect(page.locator(".toast").last()).toContainText("文字检测 已下载");
    await expect(page.locator("#status")).toContainText("需下载模型");

    await row("mathcraft-text-rec").locator(".model-download").click();
    await expect(row("mathcraft-text-rec").locator(".model-state")).toHaveText("已下载");
    await expect(page.locator("#status")).toContainText("状态: 就绪"); // text mode is now complete

    const calls = await mock<{ cmd: string; args: { model: string } }[]>(page, "calls");
    expect(calls.filter((c) => c.cmd === "download_model").map((c) => c.args.model)).toEqual([
      "mathcraft-text-det",
      "mathcraft-text-rec",
    ]);
  });

  test("a failed download is shown on its row and can be retried", async ({ page }) => {
    await open(page, { models: { formula: false, mixed: false, text: false }, failDownloads: ["mathcraft-formula-det"] });
    await page.click("#settings-btn");
    const row = page.locator('.model-row[data-model="mathcraft-formula-det"]');
    await row.locator(".model-download").click();
    await expect(row.locator(".model-state")).toHaveText("下载失败");
    await expect(row).toHaveClass(/is-failed/);
    await expect(page.locator(".toast.error")).toContainText("下载失败：network unreachable");
    await expect(row.locator(".model-download")).toBeEnabled();
    await row.locator(".model-download").click();
    await expect(row.locator(".model-state")).toHaveText("已下载");
  });

  test("a download keeps going after Settings closes and shows when reopened", async ({ page }) => {
    await open(page, { models: { formula: false, mixed: false, text: false }, downloadDelay: 250 });
    await page.click("#settings-btn");
    const row = () => page.locator('.model-row[data-model="mathcraft-formula-rec"]');
    await row().locator(".model-download").click();
    await expect(row()).toHaveClass(/is-downloading/);
    await page.locator("#settings-form").getByText("取消", { exact: true }).click();
    await page.click("#settings-btn");
    await expect(row()).toHaveClass(/is-downloading/);
    await expect(row().locator(".model-download")).toBeDisabled();
    await expect(row().locator(".model-state")).toHaveText("已下载", { timeout: 5000 });
    await expect(page.locator("#status")).toContainText("状态: 就绪");
  });

  test("models fetched by the recognition prompt also update the list", async ({ page }) => {
    await open(page, { models: { formula: false, mixed: false, text: false } });
    await page.click("#capture-btn");
    await page.click("#models-start");
    await expect(page.locator("#models-dialog")).toBeHidden();
    await page.click("#settings-btn");
    await expect(page.locator('.model-row[data-model="mathcraft-formula-rec"] .model-state')).toHaveText("已下载");
    await expect(page.locator("#models-count")).toHaveText("已下载 1/4");
  });

  test("downloaded models get a red delete button on the right", async ({ page }) => {
    await open(page, { models: { formula: true, mixed: false, text: false } });
    await page.click("#settings-btn");
    const rows = page.locator("#builtin-fields .model-row");
    await expect(rows.locator(".model-delete:visible")).toHaveCount(1);
    const del = page.locator('.model-row[data-model="mathcraft-formula-rec"] .model-delete');
    await expect(del).toBeVisible();
    await expect(del).toHaveText("删除");
    // Rightmost control in the row, after 重新下载.
    const [dl, rm] = await Promise.all([
      page.locator('.model-row[data-model="mathcraft-formula-rec"] .model-download').boundingBox(),
      del.boundingBox(),
    ]);
    expect(rm!.x).toBeGreaterThan(dl!.x + dl!.width - 1);
    // Red: the theme's danger color, in light and dark.
    const danger = () =>
      page.evaluate(() => {
        const probe = document.createElement("span");
        probe.style.color = "var(--danger)";
        document.body.append(probe);
        const c = getComputedStyle(probe).color;
        probe.remove();
        return c;
      });
    await expect.poll(async () => (await del.evaluate((el) => getComputedStyle(el).color)) === (await danger())).toBe(true);
    expect(await del.evaluate((el) => getComputedStyle(el).color)).toBe("rgb(180, 35, 24)");
    await page.locator("#settings-form .field").getByText("深色", { exact: true }).click();
    await expect.poll(async () => del.evaluate((el) => getComputedStyle(el).color)).toBe("rgb(255, 138, 128)");
    // Not-downloaded models have no delete button.
    await expect(page.locator('.model-row[data-model="mathcraft-text-det"] .model-delete')).toBeHidden();
  });

  test("deleting a model asks first, then marks it not downloaded", async ({ page }) => {
    await open(page); // all four installed, formula mode ready
    await page.click("#settings-btn");
    const row = page.locator('.model-row[data-model="mathcraft-formula-rec"]');
    await row.locator(".model-delete").click();
    await expect(page.locator("#ask-text")).toHaveText(
      "删除“公式识别”模型？公式、混合识别将无法使用，需要时可重新下载（104 MB）。",
    );
    await expect(page.locator("#ask-ok")).toHaveText("删除");
    await expect(page.locator("#ask-ok")).toHaveClass(/destructive/);

    await page.locator("#ask-dialog").getByText("取消", { exact: true }).click();
    await expect(row.locator(".model-state")).toHaveText("已下载");
    expect((await mock<{ cmd: string }[]>(page, "calls")).some((c) => c.cmd === "delete_model")).toBe(false);

    await row.locator(".model-delete").click();
    await page.click("#ask-ok");
    await expect(row.locator(".model-state")).toHaveText("未下载");
    await expect(row.locator(".model-delete")).toBeHidden();
    await expect(row.locator(".model-download")).toHaveText("下载");
    await expect(page.locator("#models-count")).toHaveText("已下载 3/4");
    await expect(page.locator(".toast").last()).toContainText("公式识别 已删除");
    await expect(page.locator("#settings-dialog")).toBeVisible(); // still in Settings
    await expect(page.locator("#status")).toContainText("需下载模型"); // formula mode lost its model
    const calls = await mock<{ cmd: string; args: { model: string } }[]>(page, "calls");
    expect(calls.filter((c) => c.cmd === "delete_model").map((c) => c.args.model)).toEqual(["mathcraft-formula-rec"]);

    // Download it again from the same row.
    await row.locator(".model-download").click();
    await expect(row.locator(".model-state")).toHaveText("已下载");
    await expect(row.locator(".model-delete")).toBeVisible();
  });

  test("delete is unavailable while that model re-downloads", async ({ page }) => {
    await open(page, { downloadDelay: 300 });
    await page.click("#settings-btn");
    const row = page.locator('.model-row[data-model="mathcraft-text-rec"]');
    await row.locator(".model-download").click(); // 重新下载
    await expect(row).toHaveClass(/is-downloading/);
    await expect(row.locator(".model-delete")).toBeDisabled();
    await expect(row.locator(".model-state")).toHaveText("已下载", { timeout: 5000 });
    await expect(row.locator(".model-delete")).toBeEnabled();
  });

  test("a failed delete is reported and keeps the model", async ({ page }) => {
    await open(page, { deleteFails: true });
    await page.click("#settings-btn");
    const row = page.locator('.model-row[data-model="mathcraft-text-det"]');
    await row.locator(".model-delete").click();
    await page.click("#ask-ok");
    await expect(page.locator(".toast.error")).toContainText("删除失败：Operation not permitted");
    await expect(row.locator(".model-state")).toHaveText("已下载");
    await expect(row.locator(".model-delete")).toBeEnabled();
  });

  test("model rows are translated", async ({ page }) => {
    await open(page, { settings: { language: "en" }, models: { formula: true, mixed: false, text: false } });
    await page.click("#settings-btn");
    await expect(page.locator("#models-count")).toHaveText("1 of 4 downloaded");
    const det = page.locator('.model-row[data-model="mathcraft-text-det"]');
    await expect(det.locator(".model-name")).toHaveText("Text detection");
    await expect(det.locator(".model-use")).toHaveText("Used by: Mixed, Text");
    await expect(det.locator(".model-download")).toHaveText("Download");
    await expect(page.locator('.model-row[data-model="mathcraft-formula-rec"] .model-delete')).toHaveText("Delete");
  });

  test("external model fields appear and connection can be tested", async ({ page }) => {
    await open(page);
    await page.click("#settings-btn");
    await expect(page.locator("#external-fields")).toBeHidden();
    await page.getByText("外部模型（OpenAI 兼容接口）").click();
    await expect(page.locator("#external-fields")).toBeVisible();
    await page.click("#test-external");
    await expect(page.locator("#test-result")).toContainText("连接失败：external model name is required");
    await page.fill("input[name=external_model]", "qwen2.5vl:7b");
    await page.click("#test-external");
    await expect(page.locator("#test-result")).toContainText("连接成功：OK (3 models)");
    await page.locator("#settings-form").getByText("保存", { exact: true }).click();
    await expect(page.locator("#status")).toContainText("外部模型 qwen2.5vl:7b");
  });

  test("hotkey recorder validates and saves", async ({ page }) => {
    await open(page);
    await page.click("#hotkey-btn");
    await page.keyboard.press("k");
    await expect(page.locator("#hotkey-hint")).toContainText("修饰键");
    await page.keyboard.press("Control+Shift+KeyK");
    await expect(page.locator("#hotkey-capture")).toHaveText("⌃⇧K");
    await page.click("#hotkey-save");
    await expect(page.locator(".toast")).toContainText("⌃⇧K");
    await expect.poll(() => mock<string>(page, "settings.hotkey")).toBe("Control+Shift+K");
    await expect(page.locator("#capture-kbd")).toHaveText("⌃⇧K");
  });

  test("recorder still hears keys after clicking inside the dialog (WebKit focus regression)", async ({ page }) => {
    await open(page);
    await page.click("#hotkey-btn");
    await page.click("#hotkey-capture"); // WebKit does not focus a clicked <button>
    await page.keyboard.press("Control+Shift+KeyK");
    await expect(page.locator("#hotkey-capture")).toHaveText("⌃⇧K");
    await page.click("#hotkey-dialog h3");
    await page.keyboard.press("Meta+Alt+KeyJ");
    await expect(page.locator("#hotkey-capture")).toHaveText("⌥⌘J");
    await page.click("#hotkey-reset");
    await expect(page.locator("#hotkey-capture")).toHaveText("⌘F");
  });

  test("can record ⌘F itself and the global hotkey is paused while recording", async ({ page }) => {
    await open(page, { settings: { hotkey: "Control+Shift+K" } });
    await page.click("#hotkey-btn");
    await expect.poll(() => mock<boolean>(page, "hotkeySuspended")).toBe(true);
    await page.keyboard.press("Meta+KeyF");
    await expect(page.locator("#hotkey-capture")).toHaveText("⌘F");
    await page.click("#hotkey-save");
    await expect.poll(() => mock<string>(page, "settings.hotkey")).toBe("CommandOrControl+F");
    await expect.poll(() => mock<boolean>(page, "hotkeySuspended")).toBe(false);
  });

  test("cancelling the recorder restores the global hotkey", async ({ page }) => {
    await open(page);
    await page.click("#hotkey-btn");
    await expect.poll(() => mock<boolean>(page, "hotkeySuspended")).toBe(true);
    await page.keyboard.press("Meta+Alt+KeyL");
    await page.keyboard.press("Escape");
    await expect(page.locator("#hotkey-dialog")).toBeHidden();
    await expect.poll(() => mock<boolean>(page, "hotkeySuspended")).toBe(false);
    expect(await mock<string>(page, "settings.hotkey")).toBe("CommandOrControl+F");
  });

  test("buttons in the recorder still work from the keyboard", async ({ page }) => {
    await open(page);
    await page.click("#hotkey-btn");
    await page.keyboard.press("Meta+Shift+KeyM");
    await page.focus("#hotkey-save");
    await page.keyboard.press("Enter");
    await expect.poll(() => mock<string>(page, "settings.hotkey")).toBe("CommandOrControl+Shift+M");
  });

  test("menu bar “Settings…” opens the settings sheet", async ({ page }) => {
    await open(page);
    await page.click("#favorites-btn");
    await page.evaluate(() => (window as any).__mock.emit("open-settings", null));
    await expect(page.locator("#settings-dialog")).toBeVisible();
    await expect(page.locator("#favorites-dialog")).toBeHidden();
  });

  test("a hotkey the system refuses is reported", async ({ page }) => {
    await open(page);
    await page.click("#hotkey-btn");
    await page.keyboard.press("Meta+Alt+KeyQ");
    await page.click("#hotkey-save");
    await expect(page.locator(".toast.error")).toContainText("快捷键不可用");
    await expect.poll(() => mock<string>(page, "settings.hotkey")).toBe("CommandOrControl+F");
  });
});

test.describe("handwriting", () => {
  test("drawing triggers formula recognition and insert", async ({ page }) => {
    await open(page, { outcomes: [{ text: "\\alpha" }] });
    await page.click("#handwriting-btn");
    const box = (await page.locator("#hand-canvas").boundingBox())!;
    await page.mouse.move(box.x + 60, box.y + 60);
    await page.mouse.down();
    for (let i = 1; i <= 10; i++) await page.mouse.move(box.x + 60 + i * 12, box.y + 60 + (i % 3) * 10);
    await page.mouse.up();
    await expect(page.locator("#hand-text")).toHaveValue("\\alpha", { timeout: 5000 });
    await expect(page.locator("#hand-preview mjx-container")).toBeVisible();
    const call = (await mock<{ cmd: string; args: { mode: string } }[]>(page, "calls")).find((c) => c.cmd === "recognize_image")!;
    expect(call.args.mode).toBe("formula");
    await page.click("#hand-insert");
    await page.locator("#hand-dialog").getByText("关闭", { exact: true }).click();
    await expect(page.locator("#editor")).toHaveValue("\\alpha");
  });

  test("clearing the canvas clears the result", async ({ page }) => {
    await open(page, { outcomes: [{ text: "x" }] });
    await page.click("#handwriting-btn");
    const box = (await page.locator("#hand-canvas").boundingBox())!;
    await page.mouse.move(box.x + 50, box.y + 50);
    await page.mouse.down();
    await page.mouse.move(box.x + 120, box.y + 90);
    await page.mouse.up();
    await expect(page.locator("#hand-text")).toHaveValue("x", { timeout: 5000 });
    await page.click("#hand-clear");
    await expect(page.locator("#hand-text")).toHaveValue("");
    await expect(page.locator("#hand-hint")).toBeVisible();
  });
});

test.describe("onboarding", () => {
  test("shows on first launch and can be completed", async ({ page }) => {
    await open(page, { settings: { onboarding_done: false } });
    const dlg = page.locator("#onboarding-dialog");
    await expect(dlg).toBeVisible();
    await expect(dlg.locator("li.on h3")).toHaveText("选择识别类型");
    for (let i = 0; i < 3; i++) await page.click("#onboarding-next");
    await expect(dlg.locator("li.on h3")).toHaveText("图片、PDF 与手写");
    await page.click("#onboarding-next");
    await expect(dlg).toBeHidden();
    await expect.poll(() => mock<boolean>(page, "settings.onboarding_done")).toBe(true);
  });
});
