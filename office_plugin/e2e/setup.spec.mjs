// The Setup window (app/ui, built into build/ui) in WebKit, with the Tauri
// bridge replaced by a mock of the host commands.
import { expect, test } from "@playwright/test";
import { existsSync, readFileSync } from "node:fs";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";

const UI = fileURLToPath(new URL("../build/ui/", import.meta.url));
const ORIGIN = "https://setup.test";
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".png": "image/png" };

const STATUS = {
  version: "1.0.0",
  installed: false,
  running: false,
  reachable: false,
  certificate: { exists: false, trusted: false },
  hosts: [
    { id: "word", app: true, ready: true, sideloaded: false },
    { id: "powerpoint", app: true, ready: false, sideloaded: false },
  ],
  model: { source: null, downloadBytes: 108795631 },
  location: null,
  url: "https://localhost:28766",
};

function bridge({ status, failInstall }) {
  const callbacks = new Map();
  const listeners = new Map();
  let next = 1;
  const state = { status, calls: [] };
  window.__setup = state;
  const emit = (event, payload) => (listeners.get(event) || []).forEach((id) => callbacks.get(id)?.({ event, id, payload }));
  const installed = () => ({
    ...state.status,
    installed: true,
    running: true,
    reachable: true,
    certificate: { exists: true, trusted: true },
    hosts: state.status.hosts.map((h) => ({ ...h, sideloaded: h.ready })),
  });
  const handlers = {
    status: () => state.status,
    install: () => {
      if (failInstall) throw "TRUST_DENIED:security 失败：User canceled";
      return (state.status = installed());
    },
    uninstall: () => (state.status = { ...state.status, installed: false, running: false, reachable: false, certificate: { exists: false, trusted: false } }),
    download_model: async () => {
      for (const done of [0.25, 0.5, 1]) {
        emit("model-progress", { downloaded: Math.round(108795631 * done), total: 108795631 });
        await new Promise((r) => setTimeout(r, 20));
      }
      return (state.status = { ...state.status, model: { ...state.status.model, source: "addin" } });
    },
    open_help: () => null,
    "plugin:event|listen": ({ event, handler }) => {
      listeners.set(event, [...(listeners.get(event) || []), handler]);
      return handler;
    },
    "plugin:event|unlisten": () => null,
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback(cb) {
      const id = next++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback: (id) => callbacks.delete(id),
    async invoke(cmd, args = {}) {
      state.calls.push(cmd);
      await new Promise((r) => setTimeout(r, 5));
      return handlers[cmd](args);
    },
  };
}

async function open(page, { status = STATUS, failInstall = false } = {}) {
  test.skip(!existsSync(join(UI, "index.html")), "run `npm run ui` first");
  await page.route(`${ORIGIN}/**`, (route) => {
    const path = new URL(route.request().url()).pathname.replace(/^\//, "") || "index.html";
    const file = join(UI, path);
    if (!existsSync(file)) return route.fulfill({ status: 404 });
    return route.fulfill({ body: readFileSync(file), contentType: TYPES[extname(file)] ?? "application/octet-stream" });
  });
  await page.addInitScript(bridge, { status, failInstall });
  await page.goto(`${ORIGIN}/index.html`);
  await expect(page.locator("#status li")).toHaveCount(5);
}

test.use({ viewport: { width: 560, height: 440 }, locale: "zh-CN" });

test("installs, then reports each host", async ({ page }) => {
  await open(page);
  await expect(page.locator("#install")).toHaveText("安装");
  await expect(page.locator("#uninstall")).toBeHidden();
  await expect(page.locator("#status li").nth(3)).toContainText("请先打开一次 PowerPoint");
  await page.screenshot({ path: "test-results/setup-before.png" });
  await page.click("#install");
  await expect(page.locator("#message")).toContainText("安装完成");
  await expect(page.locator("#status li").first()).toHaveClass(/ok/);
  await expect(page.locator("#status li").nth(2)).toContainText("在“开始 › 加载项”中选择 LaTeXSnipper");
  await expect(page.locator("#install")).toHaveText("安装 / 修复");
  await expect(page.locator("#uninstall")).toBeVisible();
});

test("downloads the model with progress", async ({ page }) => {
  await open(page);
  const row = page.locator("#status li").nth(4);
  await expect(row).toContainText("未下载（109 MB）");
  await row.getByRole("button", { name: "下载" }).click();
  await expect(page.locator("#message")).toContainText("模型已下载");
  await expect(row).toContainText("已下载");
  await expect(row.getByRole("button")).toHaveCount(0);
  await expect(page.locator("#progress")).toBeHidden();
});

test("uninstall asks for a second click", async ({ page }) => {
  await open(page, { status: { ...STATUS, installed: true, running: true, reachable: true, certificate: { exists: true, trusted: true } } });
  await page.click("#uninstall");
  await expect(page.locator("#uninstall")).toHaveText("再点一次确认卸载");
  expect(await page.evaluate(() => window.__setup.calls.includes("uninstall"))).toBe(false);
  await page.click("#uninstall");
  await expect(page.locator("#message")).toContainText("已卸载");
  await expect(page.locator("#install")).toHaveText("安装");
});

test("explains a refused certificate", async ({ page }) => {
  await open(page, { failInstall: true });
  await page.click("#install");
  await expect(page.locator("#message")).toContainText("没有信任本地证书");
  await expect(page.locator("#message")).toHaveClass(/error/);
  await expect(page.locator("#install")).toBeEnabled();
});

test("refuses to install from the disk image", async ({ page }) => {
  await open(page, { status: { ...STATUS, location: "LOCATION_DMG" } });
  await expect(page.locator("#install")).toBeDisabled();
  await expect(page.locator("#message")).toContainText("拖到“应用程序”文件夹");
});

test.describe("English", () => {
  test.use({ locale: "en-US" });
  test("follows the system language", async ({ page }) => {
    await open(page);
    await expect(page.locator("h1 + .intro")).toContainText("the LaTeXSnipper desktop app is not needed");
    await expect(page.locator("#install")).toHaveText("Install");
    await expect(page.locator("#status li").nth(4)).toContainText("Not downloaded (109 MB)");
  });
});
