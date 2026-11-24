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

  test("live preview centers a lone formula at a fixed size", async ({ page }) => {
    const box = async () => {
      const [paper, svg] = await Promise.all([page.locator("#preview-paper").boundingBox(), page.locator("#preview svg").boundingBox()]);
      return { paper: paper!, svg: svg! };
    };
    const expectCentered = async () => {
      const { paper, svg } = await box();
      expect(Math.abs(svg.x + svg.width / 2 - (paper.x + paper.width / 2))).toBeLessThan(3);
      expect(Math.abs(svg.y + svg.height / 2 - (paper.y + paper.height / 2))).toBeLessThan(3);
      return svg;
    };
    await open(page);
    // Mixed recognition returns a lone formula wrapped in $$…$$; it previews like a bare formula.
    await page.fill("#editor", "$$\n\\frac{a+b}{c^2} = \\sum_{i=1}^{n} x_i\n$$");
    await expect(page.locator("#preview > mjx-container svg")).toBeVisible();
    await expect(page.locator("#preview p")).toHaveCount(0);
    await expect(page.locator("#preview")).toHaveCSS("font-size", "32px");
    const before = await expectCentered();
    await page.setViewportSize({ width: 960, height: 620 });
    await expect.poll(async () => Math.round((await box()).paper.width)).toBeLessThan(700);
    const after = await expectCentered();
    expect(Math.abs(after.width - before.width)).toBeLessThan(1);
    expect(Math.abs(after.height - before.height)).toBeLessThan(1);
  });
});

test.describe("result window (capture while the main window is hidden)", () => {
  async function openResult(page: Page, seed: Seed): Promise<void> {
    await page.addInitScript((s) => ((window as unknown as { __MOCK_SEED__: Seed }).__MOCK_SEED__ = s), seed);
    await page.addInitScript({ path: path.join(here, "tauri-mock.js") });
    await page.goto("/result.html");
  }

  test("shows the result alone, then confirm copies, records history and tells the main window", async ({ page }) => {
    await openResult(page, { pendingResult: { text: "\\sqrt{x}" } });
    await expect(page.locator("#result-text")).toHaveValue("\\sqrt{x}");
    await expect(page.locator("#result-preview mjx-container")).toBeVisible();
    await expect.poll(() => mock<boolean>(page, "resultShown")).toBe(true);
    await expect(page.locator("#result-warn")).toBeHidden();
    await page.fill("#result-text", "\\sqrt{y}");
    await page.click("#result-confirm");
    await expect.poll(() => mock<boolean>(page, "resultShown")).toBe(false);
    expect(await mock<string>(page, "clipboard")).toBe("\\sqrt{y}");
    expect(await mock<{ text: string }[]>(page, "history")).toEqual([{ text: "\\sqrt{y}", name: "", kind: "mathcraft" }]);
    const sent = await mock<{ event: string; payload: { text: string } }[]>(page, "emitted");
    expect(sent.map((e) => [e.event, e.payload.text])).toEqual([["result-confirmed", "\\sqrt{y}"]]);
  });

  test("Escape cancels without copying or recording", async ({ page }) => {
    await openResult(page, { pendingResult: { text: "a", score: 0.4, flags: ["unbalanced_group"] }, settings: { language: "en" } });
    await expect(page.locator("#result-warn")).toContainText("unbalanced_group");
    await expect(page.locator("#result-confirm")).toHaveText("Confirm and copy");
    await expect.poll(() => mock<boolean>(page, "resultShown")).toBe(true);
    await page.keyboard.press("Escape");
    await expect.poll(() => mock<boolean>(page, "resultShown")).toBe(false);
    expect(await mock<string>(page, "clipboard")).toBe("");
    expect(await mock<unknown[]>(page, "history")).toEqual([]);
  });

  test("the main window picks up a result confirmed there", async ({ page }) => {
    await open(page);
    await page.evaluate(() =>
      (window as any).__mock.emit("result-confirmed", { text: "e^{x}", kind: "mathcraft", history: [{ text: "e^{x}", name: "", kind: "mathcraft" }] }),
    );
    await expect(historyRows(page)).toHaveCount(1);
    await expect(page.locator("#editor")).toHaveValue("e^{x}");
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

  test("image OCR stays available without native screen capture", async ({ page }) => {
    await open(page, { capture_supported: false, outcomes: [{ text: "x^{2}" }] });
    await expect(page.locator("#capture-btn")).toBeHidden();
    await expect(page.locator("#hotkey-btn")).toBeHidden();
    // The remaining tools share the row with no empty slot.
    const row = (await page.locator(".side-tools").boundingBox())!;
    const last = (await page.locator("#settings-btn").boundingBox())!;
    expect(row.x + row.width - (last.x + last.width)).toBeLessThan(1);
    await page.setInputFiles("#image-input", FIXTURE);
    await expect(page.locator("#result-text")).toHaveValue("x^{2}");
  });

  test("empty and failing recognitions show actionable messages", async ({ page }) => {
    await open(page, { outcomes: [{ error: "EMPTY_RESULT" }, { error: "decoder crashed" }] });
    await page.click("#capture-btn");
    await expect(page.locator(".toast.error")).toContainText("未检测到可识别内容");
    await page.click("#capture-btn");
    await expect(page.locator(".toast.error").last()).toContainText("识别失败：decoder crashed");
  });

  test("missing Screen Recording permission explains itself and opens System Settings", async ({ page }) => {
    await open(page, { outcomes: [{ error: "SCREEN_PERMISSION" }, { error: "SCREEN_PERMISSION" }] });
    await page.click("#capture-btn");
    await expect(page.locator("#ask-dialog")).toBeVisible();
    await expect(page.locator("#ask-text")).toContainText("屏幕录制");
    await page.click("#ask-dialog button[value=cancel]");
    await expect(page.locator("#ask-dialog")).toBeHidden();
    await page.click("#capture-btn");
    await page.click("#ask-ok");
    await expect(page.locator("#ask-dialog")).toBeHidden();
    await expect
      .poll(async () => (await mock<{ cmd: string }[]>(page, "calls")).filter((c) => c.cmd === "open_screen_settings").length)
      .toBe(1);
    await expect(page.locator(".toast.error")).toHaveCount(0);
  });

  test("missing models open the download sheet and progress to ready", async ({ page }) => {
    await open(page, { models: { formula: false, mixed: false, text: false } });
    await expect(page.locator("#status")).toContainText("需下载模型");
    await page.click("#capture-btn");
    await expect(page.locator("#models-dialog")).toBeVisible();
    // The first recognition fetches the Int8 formula recognizer (about 32 MB unpacked).
    await expect(page.locator("#models-text")).toContainText("32 MB");
    await page.click("#models-start");
    await expect(page.locator("#models-dialog")).toBeHidden();
    await expect(page.locator("#status")).toContainText("就绪");
    const calls = await mock<{ cmd: string; args: { mode: string } }[]>(page, "calls");
    expect(calls.filter((c) => c.cmd === "download_models").map((c) => c.args.mode)).toEqual(["formula"]);
    expect(await mock<boolean>(page, "installed.has('mathcraft-formula-rec-int8')")).toBe(true);
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

test.describe("recognition type dropdown", () => {
  test("lives in the main window, styled by the app, and saves the choice", async ({ page }) => {
    await open(page, { models: { formula: true, mixed: true, text: true } });
    const trigger = page.locator("#mode-trigger");
    const list = page.locator("#mode-list");
    // The app's own control, not a native <select>; Settings no longer has the radios.
    await expect(page.locator("#mode-row select")).toHaveCount(0);
    await expect(page.locator("#settings-form input[name=mode]")).toHaveCount(0);
    await expect(trigger).toHaveText("公式");
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await expect(list).toBeHidden();

    await trigger.click();
    await expect(list).toBeVisible();
    await expect(trigger).toHaveAttribute("aria-expanded", "true");
    const options = list.getByRole("option");
    await expect(options.locator(".select-option-name")).toHaveText(["公式", "混合", "纯文字"]);
    await expect(options.nth(0)).toHaveAttribute("aria-selected", "true");
    await expect(options.nth(1)).toHaveAttribute("aria-selected", "false");

    await options.nth(1).click();
    await expect(list).toBeHidden();
    await expect(trigger).toHaveText("混合");
    await expect(trigger).toBeFocused();
    await expect(page.locator("#status")).toContainText("MathCraft 混合");
    await expect.poll(() => mock<string>(page, "settings.mode")).toBe("mixed");

    // The chosen type is what captures use.
    await page.evaluate(() => (window as any).__mock.emit("hotkey-capture", null));
    await expect
      .poll(async () => (await mock<{ cmd: string; args: { mode: string } }[]>(page, "calls")).filter((c) => c.cmd === "capture").map((c) => c.args.mode))
      .toEqual(["mixed"]);
  });

  test("works from the keyboard and closes on Escape or a click elsewhere", async ({ page }) => {
    await open(page, { models: { formula: true, mixed: true, text: true } });
    const trigger = page.locator("#mode-trigger");
    const list = page.locator("#mode-list");
    await trigger.focus();
    await page.keyboard.press("ArrowDown");
    await expect(list).toBeVisible();
    await expect(list).toBeFocused();
    await expect(list.locator("li.active")).toHaveText(/^公式/); // starts on the current type
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await expect(list.locator("li.active")).toHaveText(/^纯文字/);
    await expect(list).toHaveAttribute("aria-activedescendant", "mode-option-text");
    await page.keyboard.press("ArrowDown"); // already last: stays
    await expect(list.locator("li.active")).toHaveText(/^纯文字/);
    await page.keyboard.press("Escape");
    await expect(list).toBeHidden();
    await expect(trigger).toBeFocused();
    await expect(trigger).toHaveText("公式"); // Escape changes nothing

    await page.keyboard.press("Enter");
    await expect(list).toBeVisible();
    await page.keyboard.press("End");
    await page.keyboard.press("Enter");
    await expect(trigger).toHaveText("纯文字");
    await expect.poll(() => mock<string>(page, "settings.mode")).toBe("text");

    await trigger.click();
    await expect(list).toBeVisible();
    await page.locator("#editor").click();
    await expect(list).toBeHidden();
  });

  test("is translated", async ({ page }) => {
    await open(page, { settings: { language: "en" } });
    await expect(page.locator("#mode-caption")).toHaveText("Recognition type");
    await page.click("#mode-trigger");
    await expect(page.locator("#mode-list .select-option-name")).toHaveText(["Formula", "Mixed", "Text"]);
    await expect(page.locator("#mode-list .select-option-hint").nth(1)).toHaveText("Text with formulas, as Markdown");
  });
});

test.describe("updates", () => {
  test("检查更新 reports an up-to-date app and a failed check", async ({ page }) => {
    await open(page);
    await page.click("#settings-btn");
    await expect(page.locator("#update-btn")).toHaveText("检查更新");
    await page.click("#update-btn");
    await expect(page.locator(".toast").last()).toContainText("已是最新版本");
    await expect(page.locator("#update-btn")).toHaveText("检查更新");
    await page.evaluate(() => ((window as unknown as { __mock: { updateCheckFails: boolean } }).__mock.updateCheckFails = true));
    await page.click("#update-btn");
    await expect(page.locator(".toast.error").last()).toContainText("检查更新失败：network unreachable");
    await expect(page.locator("#settings-dialog")).toBeVisible();
  });

  test("a newer release turns the button into 更新, which downloads and restarts", async ({ page }) => {
    await open(page, { latestRelease: "1.4.0", updateDelay: 60 });
    await page.click("#settings-btn");
    await page.click("#update-btn");
    await expect(page.locator("#update-btn")).toHaveText("更新到 1.4.0");
    await expect(page.locator("#update-btn")).toHaveClass(/primary/);
    await page.click("#update-btn");
    await expect(page.locator("#update-btn")).toHaveText(/正在下载 \d+%/);
    await expect(page.locator("#update-btn")).toBeDisabled();
    await expect.poll(() => mock<boolean>(page, "restarted")).toBe(true);
    await expect(page.locator("#settings-dialog")).toBeVisible();
  });

  test("a background download shows as ready; a failed install is reported", async ({ page }) => {
    await open(page, { update: { phase: "available", version: "1.4.0", installs_on_quit: false }, updateInstallFails: true });
    await page.click("#settings-btn");
    await expect(page.locator("#update-btn")).toHaveText("更新到 1.4.0");
    await page.evaluate(() =>
      (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit("update-status", {
        phase: "ready",
        version: "1.4.0",
        installs_on_quit: true,
      }),
    );
    await expect(page.locator("#update-btn")).toHaveText("重启以完成更新");
    await expect(page.locator("#update-btn")).toHaveAttribute("title", "1.4.0 已下载，退出 LaTeXSnipper 时会自动安装");
    await page.evaluate(() => {
      const m = (window as unknown as { __mock: { update: unknown } }).__mock;
      m.update = { phase: "available", version: "1.4.0", installs_on_quit: false };
    });
    await page.click("#update-btn");
    await expect(page.locator(".toast.error").last()).toContainText("更新失败：signature mismatch");
    await expect(page.locator("#update-btn")).toHaveText("更新到 1.4.0");
  });

  test("the first launch after an update shows its release notes once", async ({ page }) => {
    await open(page, {
      settings: { onboarding_done: true },
      changelog: { version: "1.4.0", notes: "### 改进与修复\n\n- **预览**：固定大小\n- <b>不是标签</b>" },
    });
    await expect(page.locator("#changelog-dialog")).toBeVisible();
    await expect(page.locator("#changelog-title")).toHaveText("已更新到 LaTeXSnipper 1.4.0");
    await expect(page.locator("#changelog-body h4")).toHaveText("改进与修复");
    await expect(page.locator("#changelog-body li strong")).toHaveText("预览");
    await expect(page.locator("#changelog-body li").last()).toHaveText("<b>不是标签</b>");
    await page.click("#changelog-ok");
    await expect(page.locator("#changelog-dialog")).toBeHidden();
    await expect.poll(() => mock<boolean>(page, "changelogDismissed")).toBe(true);
  });

  test("no release notes without an update", async ({ page }) => {
    await open(page, { settings: { onboarding_done: true } });
    await expect(page.locator("#changelog-dialog")).toBeHidden();
  });
});

test.describe("settings", () => {
  test("switching mode, theme and language persists and re-renders", async ({ page }) => {
    await open(page);
    await page.click("#mode-trigger");
    await page.locator("#mode-list").getByText("混合", { exact: true }).click();
    await page.click("#settings-btn");
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

  test("built-in backend lists the four Int8 models with their download state", async ({ page }) => {
    await open(page, { models: { formula: true, mixed: false, text: false } });
    await page.click("#settings-btn");
    const panel = page.locator("#builtin-fields");
    await expect(panel).toBeVisible();
    await expect(page.locator("#models-count")).toHaveText("已下载 1/4");
    const rows = panel.locator(".model-row");
    await expect(rows).toHaveCount(4);
    await expect(rows.locator(".model-name")).toHaveText(["公式检测", "公式识别", "文字检测", "文字识别"]);
    expect(await rows.evaluateAll((els) => els.map((el) => (el as HTMLElement).dataset.model))).toEqual([
      "mathcraft-formula-det-int8",
      "mathcraft-formula-rec-int8",
      "mathcraft-text-det",
      "mathcraft-text-rec",
    ]);
    await expect(rows.locator(".model-use")).toHaveText([
      "用于：混合",
      "用于：公式、混合",
      "用于：混合、纯文字",
      "用于：混合、纯文字",
    ]);
    await expect(rows.locator(".model-size")).toHaveText(["20 MB", "24 MB", "4.3 MB", "13 MB"]);
    await expect(rows.locator(".model-state")).toHaveText(["未下载", "已下载", "未下载", "未下载"]);
    await expect(rows.locator(".model-download")).toHaveText(["下载", "重新下载", "下载", "下载"]);
    // No build to choose any more: no 选用 buttons, no Int8 tooltips.
    await expect(panel.locator(".model-pick, .model-help, input[type=radio]")).toHaveCount(0);
    await expect(panel).not.toContainText("Int8");
  });

  test("the first launch's background download shows in the status and model list", async ({ page }) => {
    await open(page, { models: { formula: false, mixed: false, text: false } });
    await expect(page.locator("#status")).toContainText("需下载模型");
    // What the backend's first-launch download sends: progress, then the state after each model.
    const emit = (event: string, payload: unknown) =>
      page.evaluate(
        ([e, p]) => (window as unknown as { __mock: { emit: (e: string, p: unknown) => void } }).__mock.emit(e as string, p),
        [event, payload] as const,
      );
    await page.evaluate(() => (window as unknown as { __mock: { downloading: Set<string> } }).__mock.downloading.add("mathcraft-formula-rec-int8"));
    await emit("model-progress", { model: "mathcraft-formula-rec-int8", downloaded: 1_000_000, total: 24_695_733 });
    await expect(page.locator("#status")).toContainText("正在下载模型…");
    await page.click("#settings-btn");
    const row = page.locator('.model-row[data-model="mathcraft-formula-rec-int8"]');
    await expect(row).toHaveClass(/is-downloading/);
    await page.evaluate(() => {
      const m = (window as unknown as {
        __mock: { installed: Set<string>; downloading: Set<string>; emit: (e: string, p: unknown) => void; modelsState: () => unknown };
      }).__mock;
      m.downloading.delete("mathcraft-formula-rec-int8");
      m.installed.add("mathcraft-formula-rec-int8");
      m.emit("models-state", m.modelsState());
    });
    await expect(row.locator(".model-state")).toHaveText("已下载");
    await expect(page.locator("#models-count")).toHaveText("已下载 1/4");
    await expect(page.locator("#status")).toContainText("状态: 就绪"); // formula mode is ready
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
    await open(page, { models: { formula: false, mixed: false, text: false }, failDownloads: ["mathcraft-formula-det-int8"] });
    await page.click("#settings-btn");
    const row = page.locator('.model-row[data-model="mathcraft-formula-det-int8"]');
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
    const row = () => page.locator('.model-row[data-model="mathcraft-formula-rec-int8"]');
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
    await expect(page.locator('.model-row[data-model="mathcraft-formula-rec-int8"] .model-state')).toHaveText("已下载");
    await expect(page.locator("#models-count")).toHaveText("已下载 1/4");
  });

  test("downloaded models get a red delete button on the right", async ({ page }) => {
    await open(page, { models: { formula: true, mixed: false, text: false } });
    await page.click("#settings-btn");
    const rows = page.locator("#builtin-fields .model-row");
    await expect(rows.locator(".model-delete:visible")).toHaveCount(1);
    const del = page.locator('.model-row[data-model="mathcraft-formula-rec-int8"] .model-delete');
    await expect(del).toBeVisible();
    await expect(del).toHaveText("删除");
    // Rightmost control in the row, after 重新下载.
    const [dl, rm] = await Promise.all([
      page.locator('.model-row[data-model="mathcraft-formula-rec-int8"] .model-download').boundingBox(),
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
    await open(page); // every model installed, formula mode ready
    await page.click("#settings-btn");
    const row = page.locator('.model-row[data-model="mathcraft-formula-rec-int8"]');
    await row.locator(".model-delete").click();
    await expect(page.locator("#ask-text")).toHaveText(
      "删除“公式识别”模型？公式、混合识别将无法使用，需要时可重新下载（24 MB）。",
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
    expect(calls.filter((c) => c.cmd === "delete_model").map((c) => c.args.model)).toEqual(["mathcraft-formula-rec-int8"]);

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
    await expect(page.locator('.model-row[data-model="mathcraft-formula-rec-int8"] .model-delete')).toHaveText("Delete");
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

test.describe("quick start spotlight tour", () => {
  type Rect = { x: number; y: number; width: number; height: number };
  const rectOf = (page: Page, sel: string) => page.locator(sel).boundingBox() as Promise<Rect>;
  const contains = (outer: Rect, inner: Rect) =>
    outer.x <= inner.x + 0.5 &&
    outer.y <= inner.y + 0.5 &&
    outer.x + outer.width >= inner.x + inner.width - 0.5 &&
    outer.y + outer.height >= inner.y + inner.height - 0.5;
  const overlaps = (a: Rect, b: Rect) =>
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
  /**
   * Wait until the hole has finished gliding and wraps exactly the given controls
   * (contains them, at most a few px of padding), and the card has come to rest:
   * same position on two reads 100 ms apart. Mid-animation frames legitimately overlap.
   */
  async function expectSpotlight(page: Page, ...targets: string[]) {
    await expect
      .poll(async () => {
        const hole = await rectOf(page, ".tour-hole");
        const rects = await Promise.all(targets.map((t) => rectOf(page, t)));
        if (!rects.every((r) => contains(hole, r))) return "hole does not contain target";
        const left = Math.min(...rects.map((r) => r.x));
        const right = Math.max(...rects.map((r) => r.x + r.width));
        if (hole.width > right - left + 16) return "hole still wider than target";
        const a = await rectOf(page, ".tour-card");
        await page.waitForTimeout(100);
        const b = await rectOf(page, ".tour-card");
        return a.x === b.x && a.y === b.y ? "settled" : "card still moving";
      })
      .toBe("settled");
  }
  const STEPS: [string, string[]][] = [
    ["截图识别", ["#capture-btn"]],
    ["图片、PDF 与手写", ["#image-btn", "#pdf-btn", "#handwriting-btn"]],
    ["识别类型", ["#mode-row"]],
    ["编辑与预览", ["#editor", "#preview-paper"]],
    ["复制与导出", ["#copy-btn", "#export-btn"]],
    ["历史记录", ["#history"]],
    ["快捷键与设置", ["#clear-history-btn", "#hotkey-btn", "#favorites-btn", "#settings-btn"]],
    ["后台运行", ["#onboarding-btn"]],
  ];

  test("starts on first launch and spotlights each real control in turn", async ({ page }) => {
    await open(page, { settings: { onboarding_done: false } });
    const tour = page.locator(".tour");
    await expect(tour).toBeVisible();
    await expect(tour).toHaveAttribute("role", "dialog");
    for (const [i, [title, targets]] of STEPS.entries()) {
      await expect(page.locator(".tour-count")).toHaveText(`${i + 1} / ${STEPS.length}`);
      await expect(page.locator(".tour-title")).toHaveText(title);
      await expectSpotlight(page, ...targets);
      // The card never covers what it is pointing at, and stays in the window.
      const card = await rectOf(page, ".tour-card");
      const hole = await rectOf(page, ".tour-hole");
      expect(overlaps(card, hole), `step ${i + 1} card overlaps its target`).toBe(false);
      expect(card.x >= 0 && card.y >= 0 && card.x + card.width <= 1160 && card.y + card.height <= 720).toBe(true);
      if (i < STEPS.length - 1) await page.click(".tour-next");
    }
    await expect(page.locator(".tour-next")).toHaveText("开始使用");
    await expect(page.locator(".tour-body")).toContainText("⌘F");
    await page.click(".tour-next");
    await expect(tour).toHaveCount(0);
    await expect.poll(() => mock<boolean>(page, "settings.onboarding_done")).toBe(true);
  });

  test("Back, arrow keys and Enter move between steps", async ({ page }) => {
    await open(page);
    await page.click("#onboarding-btn");
    await expect(page.locator(".tour-back")).toBeDisabled();
    await expect(page.locator(".tour-next")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.locator(".tour-title")).toHaveText("图片、PDF 与手写");
    await page.keyboard.press("ArrowRight");
    await expect(page.locator(".tour-title")).toHaveText("识别类型");
    await page.keyboard.press("ArrowLeft");
    await expect(page.locator(".tour-title")).toHaveText("图片、PDF 与手写");
    await page.click(".tour-back");
    await expect(page.locator(".tour-title")).toHaveText("截图识别");
    await page.keyboard.press("ArrowLeft"); // already first: stays
    await expect(page.locator(".tour-count")).toHaveText("1 / 8");
  });

  test("Skip and Esc end the tour, mark it seen and return focus", async ({ page }) => {
    await open(page, { settings: { onboarding_done: false } });
    await page.click(".tour-next");
    await page.click(".tour-skip");
    await expect(page.locator(".tour")).toHaveCount(0);
    await expect.poll(() => mock<boolean>(page, "settings.onboarding_done")).toBe(true);

    await page.focus("#onboarding-btn");
    await page.keyboard.press("Enter");
    await expect(page.locator(".tour")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator(".tour")).toHaveCount(0);
    await expect(page.locator("#onboarding-btn")).toBeFocused();
  });

  test("does not reappear once seen, and replaying it does not save settings", async ({ page }) => {
    await open(page); // onboarding_done: true
    await expect(page.locator(".tour")).toHaveCount(0);
    await page.click("#onboarding-btn");
    for (let i = 0; i < STEPS.length; i++) await page.click(".tour-next");
    await expect(page.locator(".tour")).toHaveCount(0);
    const calls = await mock<{ cmd: string }[]>(page, "calls");
    expect(calls.filter((c) => c.cmd === "save_settings")).toHaveLength(0);
  });

  test("highlighted controls cannot be clicked through the overlay", async ({ page }) => {
    await open(page);
    await page.click("#onboarding-btn");
    await expectSpotlight(page, "#capture-btn");
    const box = await rectOf(page, "#capture-btn");
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.click(600, 600); // the dimmed area does nothing either
    await expect(page.locator(".tour")).toBeVisible();
    await expect(page.locator(".tour-count")).toHaveText("1 / 8");
    const calls = await mock<{ cmd: string }[]>(page, "calls");
    expect(calls.some((c) => c.cmd === "capture")).toBe(false);
    await expect(page.locator(".tour-next")).toBeFocused();
  });

  test("Tab stays on the tour's buttons", async ({ page }) => {
    await open(page);
    await page.click("#onboarding-btn");
    await page.click(".tour-next"); // step 2: Skip, Back and Next are all enabled
    const focused = () => page.evaluate(() => document.activeElement?.className ?? "");
    const seen = new Set<string>();
    for (let i = 0; i < 6; i++) {
      await page.keyboard.press("Tab");
      seen.add(await focused());
    }
    expect([...seen].every((c) => /tour-(skip|back|next)/.test(c))).toBe(true);
    expect(seen.size).toBe(3);
  });

  test("follows the layout when the window is resized", async ({ page }) => {
    await open(page);
    await page.click("#onboarding-btn");
    for (let i = 0; i < 4; i++) await page.click(".tour-next"); // 复制与导出, top right
    await expectSpotlight(page, "#copy-btn", "#export-btn");
    await page.setViewportSize({ width: 940, height: 600 }); // the app's minimum window size
    await expectSpotlight(page, "#copy-btn", "#export-btn");
    const card = await rectOf(page, ".tour-card");
    expect(card.x + card.width).toBeLessThanOrEqual(940);
    // Every step still fits at the minimum size.
    await page.click(".tour-skip");
    await page.click("#onboarding-btn");
    for (const [i, [, targets]] of STEPS.entries()) {
      await expectSpotlight(page, ...targets);
      const c = await rectOf(page, ".tour-card");
      expect(c.x >= 0 && c.y >= 0 && c.x + c.width <= 940 && c.y + c.height <= 600, `step ${i + 1}`).toBe(true);
      expect(overlaps(c, await rectOf(page, ".tour-hole")), `step ${i + 1} overlap`).toBe(false);
      if (i < STEPS.length - 1) await page.click(".tour-next");
    }
  });

  test("the global hotkey ends the tour and captures; paste is ignored", async ({ page }) => {
    await open(page, { outcomes: [{ text: "e^{x}" }] });
    await page.click("#onboarding-btn");
    await page.evaluate(() => document.dispatchEvent(new Event("paste")));
    await expect(page.locator(".tour")).toBeVisible();
    await page.evaluate(() => (window as any).__mock.emit("hotkey-capture", null));
    await expect(page.locator(".tour")).toHaveCount(0);
    await expect(page.locator("#result-text")).toHaveValue("e^{x}");
  });

  test("menu bar Settings… closes the tour first", async ({ page }) => {
    await open(page);
    await page.click("#onboarding-btn");
    await page.evaluate(() => (window as any).__mock.emit("open-settings", null));
    await expect(page.locator(".tour")).toHaveCount(0);
    await expect(page.locator("#settings-dialog")).toBeVisible();
  });

  test("without native capture it skips the capture step and macOS-only wording", async ({ page }) => {
    // What a Windows build looks like: WebView2 reports Win32, and capture is off.
    await page.addInitScript(() => Object.defineProperty(Navigator.prototype, "platform", { get: () => "Win32" }));
    await open(page, { capture_supported: false, settings: { onboarding_done: false } });
    await expect(page.locator(".tour-count")).toHaveText("1 / 7");
    await expect(page.locator(".tour-title")).toHaveText("图片、PDF 与手写");
    await expectSpotlight(page, "#image-btn", "#pdf-btn", "#handwriting-btn");
    const titles: string[] = [];
    const bodies: string[] = [];
    for (let i = 0; i < 7; i++) {
      titles.push((await page.locator(".tour-title").textContent()) ?? "");
      bodies.push((await page.locator(".tour-body").textContent()) ?? "");
      if (i < 6) await page.click(".tour-next");
    }
    expect(titles).toEqual(["图片、PDF 与手写", "识别类型", "编辑与预览", "复制与导出", "历史记录", "收藏夹与设置", "后台运行"]);
    const all = bodies.join(" ");
    expect(all).not.toMatch(/⌘|截图快捷键|菜单栏/);
    expect(bodies[0]).toContain("Ctrl+V");
    expect(bodies[1]).not.toContain("截图");
    expect(bodies[6]).toContain("系统托盘");
    // The 5th step spotlights the visible tools only (快捷键 is hidden on this build).
    await page.click(".tour-back");
    await expectSpotlight(page, "#clear-history-btn", "#favorites-btn", "#settings-btn");
  });

  test("is translated and uses the configured shortcut", async ({ page }) => {
    await open(page, { settings: { language: "en", hotkey: "Control+Shift+K", onboarding_done: false } });
    await expect(page.locator(".tour-title")).toHaveText("Capture");
    await expect(page.locator(".tour-body")).toContainText("Press ⌃⇧K or click here");
    await expect(page.locator(".tour-skip")).toHaveText("Skip");
    await expect(page.locator(".tour-next")).toHaveText("Next");
    await page.click(".tour-next");
    await expect(page.locator(".tour-back")).toHaveText("Back");
  });
});
