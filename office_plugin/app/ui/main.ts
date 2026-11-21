// Setup window: shows what the add-in host reports and runs its commands.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { describeError, describeStatus, isChinese, text, type HostStatus, type Lang } from "./status.ts";

const lang: Lang = isChinese(navigator.language) ? "zh" : "en";
const t = (key: Parameters<typeof text>[1], vars?: Record<string, string>) => text(lang, key, vars);
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
for (const el of document.querySelectorAll<HTMLElement>("[data-t]")) el.textContent = t(el.dataset.t as never);

const list = $("status");
const message = $("message");
const progress = $<HTMLProgressElement>("progress");
const installBtn = $<HTMLButtonElement>("install");
const uninstallBtn = $<HTMLButtonElement>("uninstall");
let busy = false;
let current: HostStatus | null = null;
let confirmUntil = 0;

function say(msg: string, kind: "" | "ok" | "error" = ""): void {
  message.textContent = msg;
  message.className = `message ${kind}`;
}

function render(s: HostStatus): void {
  current = s;
  list.replaceChildren();
  for (const row of describeStatus(lang, s)) {
    const li = document.createElement("li");
    li.className = row.level;
    const dot = document.createElement("span");
    dot.className = "dot";
    const label = document.createElement("span");
    label.className = "label";
    label.textContent = row.label;
    const value = document.createElement("span");
    value.className = "value";
    value.textContent = row.value;
    li.append(dot, label, value);
    if (row.action === "download") {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "btn small";
      b.textContent = t("download");
      b.disabled = busy;
      b.addEventListener("click", downloadModel);
      li.append(b);
    } else {
      li.append(document.createElement("span"));
    }
    list.append(li);
  }
  installBtn.textContent = s.installed ? t("repair") : t("install");
  installBtn.disabled = busy || s.location !== null;
  uninstallBtn.hidden = !s.installed && !s.certificate.exists;
  uninstallBtn.disabled = busy;
  if (s.location) say(describeError(lang, s.location), "error");
}

async function refresh(): Promise<void> {
  try {
    render(await invoke<HostStatus>("status"));
  } catch (e) {
    say(describeError(lang, String(e)), "error");
  }
}

async function act(command: string, working: string, done: string): Promise<void> {
  if (busy) return;
  busy = true;
  if (current) render(current);
  say(working);
  try {
    render(await invoke<HostStatus>(command));
    say(done, "ok");
  } catch (e) {
    say(describeError(lang, String(e)), "error");
    await refresh().catch(() => undefined);
  } finally {
    busy = false;
    if (current) render(current);
  }
}

async function downloadModel(): Promise<void> {
  if (busy) return;
  progress.hidden = false;
  progress.value = 0;
  const stop = await listen<{ downloaded: number; total: number }>("model-progress", ({ payload }) => {
    if (payload.total > 0) progress.value = payload.downloaded / payload.total;
    say(t("downloading", { done: (payload.downloaded / 1e6).toFixed(0), total: (payload.total / 1e6).toFixed(0) }));
  });
  try {
    await act("download_model", t("downloadStarting"), t("downloaded"));
  } finally {
    stop();
    progress.hidden = true;
  }
}

installBtn.addEventListener("click", () => act("install", t("installing"), t("installed")));
uninstallBtn.addEventListener("click", () => {
  // Two clicks within five seconds: uninstalling also removes settings and downloaded models.
  if (Date.now() > confirmUntil) {
    confirmUntil = Date.now() + 5000;
    uninstallBtn.textContent = t("confirmUninstall");
    setTimeout(() => (uninstallBtn.textContent = t("uninstall")), 5000);
    return;
  }
  confirmUntil = 0;
  uninstallBtn.textContent = t("uninstall");
  act("uninstall", t("uninstalling"), t("uninstalled"));
});
$("help").addEventListener("click", () => invoke("open_help"));
window.addEventListener("focus", () => !busy && refresh());

refresh();
