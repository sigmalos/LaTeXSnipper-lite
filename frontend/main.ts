import { api, type ModelInfo, type ModelsState, type Recognition, type Settings, type Snapshot } from "./lib/api";
import {
  COPY_FORMATS,
  EXPORT_FORMATS,
  acceleratorFromEvent,
  debounce,
  errorCode,
  formatBytes,
  formatAccelerator,
  joinPdfPages,
  nativeTheme,
  orderedEntries,
  parsePageRange,
  percentOf,
  resolveTheme,
  toMarkdown,
  toTexDocument,
  wrapLatex,
  type CopyFormat,
  type Entry,
  type Kind,
  type Mode,
} from "./lib/format";
import { getLang, setLang, t, translateDom } from "./lib/i18n";
import { InkModel, drawStrokes, inkToPng } from "./lib/ink";
import { SpotlightTour, type TourStep } from "./lib/tour";
import { renderInto, toHtmlDocument, toMathML, toSvg } from "./lib/math";

const DEFAULT_HOTKEY = "CommandOrControl+F";
const MODEL_SIZE: Record<Mode, string> = { formula: "112 MB", mixed: "210 MB", text: "21 MB" };

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

let snap: Snapshot;
let busy = false;

// ------------------------------------------------------------------ chrome

function toast(message: string, error = false): void {
  const el = document.createElement("div");
  el.className = error ? "toast error" : "toast";
  el.textContent = message;
  $("toasts").appendChild(el);
  setTimeout(() => el.remove(), error ? 4200 : 2200);
}

// The theme on screen. Differs from snap.settings.theme while Settings previews one.
let activeTheme: Settings["theme"] = "auto";
const systemDark = matchMedia("(prefers-color-scheme: dark)");

function applyTheme(theme: Settings["theme"]): void {
  activeTheme = theme;
  document.documentElement.dataset.theme = resolveTheme(theme, systemDark.matches);
  // For "auto" the media query flips once the window follows the system again,
  // and the listener below repaints with the right palette.
  api.setWindowTheme(nativeTheme(theme)).catch(() => undefined);
}

systemDark.addEventListener("change", () => {
  document.documentElement.dataset.theme = resolveTheme(activeTheme, systemDark.matches);
});

function modeLabel(mode: Mode): string {
  return t(mode === "formula" ? "公式" : mode === "mixed" ? "混合" : "纯文字");
}

function renderStatus(): void {
  const s = snap.settings;
  const model = s.backend === "external" ? `${t("外部模型")} ${s.external_model}` : `MathCraft ${modeLabel(s.mode)}`;
  const ready = s.backend === "external" || snap.models[s.mode];
  const [cls, state] = busy ? ["busy", t("识别中…")] : ready ? ["", t("就绪")] : ["need", t("需下载模型")];
  $("status").innerHTML = "";
  const dot = document.createElement("span");
  dot.className = `dot ${cls}`;
  $("status").append(dot, `${t("当前模型")}: ${model} | ${t("状态")}: ${state}`);
  $("capture-btn").classList.toggle("busy", busy);
}

function setBusy(on: boolean): void {
  busy = on;
  renderStatus();
}

function applyLanguage(): void {
  setLang(snap.settings.language);
  document.documentElement.lang = getLang() === "zh" ? "zh-CN" : "en";
  translateDom(document);
  $("capture-kbd").textContent = formatAccelerator(snap.settings.hotkey);
  renderStatus();
  renderHistory();
  buildExportMenu();
}

// ------------------------------------------------------------------ ask dialog

function ask(text: string, okLabel: string, value?: string, destructive = false): Promise<string | null> {
  const dlg = $<HTMLDialogElement>("ask-dialog");
  const input = $<HTMLInputElement>("ask-input");
  $("ask-text").textContent = text;
  $("ask-ok").textContent = okLabel;
  $("ask-ok").classList.toggle("destructive", destructive);
  input.hidden = value === undefined;
  input.value = value ?? "";
  dlg.returnValue = "";
  dlg.showModal();
  if (value !== undefined) input.select();
  return new Promise((resolve) => {
    dlg.addEventListener("close", () => resolve(dlg.returnValue === "ok" ? input.value : null), { once: true });
  });
}

// ------------------------------------------------------------------ editor + preview

const editor = $<HTMLTextAreaElement>("editor");

async function renderPreview(target: HTMLElement, source: string, errorEl?: HTMLElement): Promise<void> {
  try {
    await renderInto(target, source);
    if (errorEl) errorEl.hidden = true;
  } catch (e) {
    if (errorEl) {
      errorEl.textContent = t("无法渲染：{error}", { error: String((e as Error).message ?? e) });
      errorEl.hidden = false;
    }
  }
}

const refreshPreview = debounce(() => renderPreview($("preview"), editor.value, $("preview-error")), 120);

function loadIntoEditor(text: string, _kind?: Kind): void {
  editor.value = text;
  refreshPreview();
}

editor.addEventListener("input", () => refreshPreview());

$("clear-preview-btn").addEventListener("click", () => {
  editor.value = "";
  refreshPreview.cancel();
  $("preview").replaceChildren();
  $("preview-error").hidden = true;
});

async function copyText(text: string, label?: string): Promise<void> {
  if (!text.trim()) return toast(t("内容为空"), true);
  await api.copy(text);
  toast(label ? t("已复制 {format}", { format: label }) : t("已复制"));
}

$("copy-btn").addEventListener("click", () => copyText(editor.value));

// ------------------------------------------------------------------ history

function entryRow(entry: Entry, index: number, list: "history" | "favorites"): HTMLLIElement {
  const li = document.createElement("li");
  li.dataset.index = String(index);
  const num = document.createElement("span");
  num.className = "num";
  num.textContent = `#${index + 1}`;
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = entry.name;
  const src = document.createElement("pre");
  src.className = "src";
  src.textContent = entry.text;
  src.title = t("载入编辑器");
  src.addEventListener("click", () => {
    loadIntoEditor(entry.text, entry.kind);
    if (list === "favorites") $<HTMLDialogElement>("favorites-dialog").close();
  });
  const acts = document.createElement("div");
  acts.className = "acts";
  const copy = iconButton("i-copy", t("复制到剪贴板"), () => copyText(entry.text));
  const more = iconButton("i-more", "…", (ev) => openItemMenu(ev as MouseEvent, entry, index, list));
  acts.append(copy, more);
  li.append(num, name, src, acts);
  li.addEventListener("contextmenu", (ev) => {
    ev.preventDefault();
    openItemMenu(ev, entry, index, list);
  });
  return li;
}

function iconButton(icon: string, title: string, onClick: (ev: Event) => void): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "btn quiet";
  b.title = title;
  b.setAttribute("aria-label", title);
  b.innerHTML = `<svg class="ico"><use href="#${icon}"/></svg>`;
  b.addEventListener("click", (ev) => {
    ev.stopPropagation();
    onClick(ev);
  });
  return b;
}

function renderHistory(): void {
  const ol = $("history");
  ol.replaceChildren();
  const reverse = snap.settings.history_reverse;
  $("order-label").textContent = t(reverse ? "最新在前" : "最早在前");
  if (!snap.history.length) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = snap.capture_supported
      ? t("还没有识别记录。按 {hotkey} 截图识别，或把图片拖到窗口里。", {
          hotkey: formatAccelerator(snap.settings.hotkey),
        })
      : t("还没有识别记录。拖入图片或粘贴截图即可识别。");
    ol.appendChild(li);
    return;
  }
  for (const { entry, index } of orderedEntries(snap.history, reverse)) ol.appendChild(entryRow(entry, index, "history"));
}

function renderFavorites(): void {
  const ol = $("favorites");
  ol.replaceChildren();
  if (!snap.favorites.length) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = t("收藏夹是空的。在历史记录的菜单里选择“收藏”。");
    ol.appendChild(li);
    return;
  }
  snap.favorites.forEach((entry, index) => ol.appendChild(entryRow(entry, index, "favorites")));
}

function closeMenus(): void {
  $("item-menu").hidden = true;
  $("export-menu").hidden = true;
}
document.addEventListener("click", closeMenus);
document.addEventListener("keydown", (e) => e.key === "Escape" && closeMenus());

function menuButton(label: string, action: () => void, disabled = false): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.role = "menuitem";
  b.textContent = label;
  b.disabled = disabled;
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    closeMenus();
    action();
  });
  return b;
}

function openItemMenu(ev: MouseEvent, entry: Entry, index: number, list: "history" | "favorites"): void {
  ev.stopPropagation();
  const menu = $("item-menu");
  const store = list === "history" ? api.history : api.favorites;
  const update = (items: Entry[]) => {
    if (list === "history") {
      snap.history = items;
      renderHistory();
    } else {
      snap.favorites = items;
      renderFavorites();
    }
  };
  const items = [
    menuButton(t("载入编辑器"), () => loadIntoEditor(entry.text, entry.kind)),
    menuButton(t("复制"), () => copyText(entry.text)),
    menuButton(t("重命名"), async () => {
      const name = await ask(t("名称"), t("保存"), entry.name);
      if (name !== null) update(await store({ op: "rename", index, name }));
    }),
  ];
  if (list === "history") {
    items.push(
      menuButton(t("收藏"), async () => {
        const before = snap.favorites.length;
        snap.favorites = await api.favorites({ op: "add", text: entry.text, kind: entry.kind, name: entry.name });
        toast(t(snap.favorites.length > before ? "已收藏" : "已在收藏夹中"));
      }),
    );
  }
  items.push(menuButton(t("删除"), async () => update(await store({ op: "remove", index }))));
  menu.replaceChildren(...items);
  menu.hidden = false;
  const { innerWidth: w, innerHeight: h } = window;
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(ev.clientX, w - r.width - 8)}px`;
  menu.style.top = `${Math.min(ev.clientY, h - r.height - 8)}px`;
}

$("order-btn").addEventListener("click", async () => {
  snap.settings = await api.saveSettings({ ...snap.settings, history_reverse: !snap.settings.history_reverse });
  renderHistory();
});

$("clear-history-btn").addEventListener("click", async () => {
  if (!snap.history.length) return;
  if ((await ask(t("确定要清空全部 {count} 条历史记录吗？", { count: snap.history.length }), t("清空全部"))) === null) return;
  snap.history = await api.history({ op: "clear" });
  renderHistory();
});

// ------------------------------------------------------------------ recognition

async function handleError(e: unknown, mode: Mode): Promise<void> {
  const { code, detail } = errorCode(e);
  if (code === "MODELS_MISSING") return openModels(mode);
  if (code === "EMPTY_RESULT") return toast(t("未检测到可识别内容"), true);
  toast(t("识别失败：{error}", { error: detail }), true);
}

async function recognizeWith(fn: () => Promise<Recognition | null>, mode: Mode): Promise<void> {
  if (busy) return;
  setBusy(true);
  try {
    const result = await fn();
    if (result) openResult(result);
  } catch (e) {
    await handleError(e, mode);
  } finally {
    setBusy(false);
  }
}

const capture = () => {
  if (tour.isOpen) tour.end(false); // the user went straight to trying it
  return recognizeWith(() => api.capture(snap.settings.mode), snap.settings.mode);
};
$("capture-btn").addEventListener("click", capture);

function readAsDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error);
    r.readAsDataURL(file);
  });
}

function recognizeBlob(file: Blob): Promise<void> {
  return recognizeWith(async () => api.recognizeImage(await readAsDataUrl(file), snap.settings.mode), snap.settings.mode);
}

const imageInput = $<HTMLInputElement>("image-input");
$("image-btn").addEventListener("click", () => imageInput.click());
imageInput.addEventListener("change", () => {
  const f = imageInput.files?.[0];
  imageInput.value = "";
  if (f) recognizeBlob(f);
});

document.addEventListener("paste", (e) => {
  if (tour.isOpen) return;
  if (document.activeElement instanceof HTMLTextAreaElement || document.activeElement instanceof HTMLInputElement) return;
  const file = [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith("image/"));
  if (file) {
    e.preventDefault();
    recognizeBlob(file);
  }
});

// result dialog
let pending: Recognition | null = null;
const resultText = $<HTMLTextAreaElement>("result-text");
const refreshResult = debounce(() => renderPreview($("result-preview"), resultText.value), 120);
resultText.addEventListener("input", refreshResult);

function openResult(r: Recognition): void {
  pending = r;
  resultText.value = r.text;
  const warn = $("result-warn");
  warn.hidden = r.score >= 0.6 && !r.flags.length;
  warn.textContent = t("低置信度，请检查结果") + (r.flags.length ? ` (${r.flags.join(", ")})` : "");
  renderPreview($("result-preview"), r.text);
  const dlg = $<HTMLDialogElement>("result-dialog");
  dlg.returnValue = "";
  dlg.showModal();
  $("result-confirm").focus();
}

$<HTMLDialogElement>("result-dialog").addEventListener("close", async () => {
  const dlg = $<HTMLDialogElement>("result-dialog");
  if (dlg.returnValue !== "confirm" || !pending) return;
  const text = resultText.value.trim();
  if (!text) return;
  await api.copy(text);
  snap.history = await api.history({ op: "add", text, kind: pending.kind });
  loadIntoEditor(text, pending.kind);
  renderHistory();
  toast(t("已确认并复制到剪贴板"));
  pending = null;
});

// ------------------------------------------------------------------ models

let downloadMode: Mode = "formula";
function openModels(mode: Mode): void {
  downloadMode = mode;
  $("models-text").textContent = t("{mode}识别需要下载 MathCraft 模型（约 {size}），只需下载一次。", {
    mode: modeLabel(mode),
    size: MODEL_SIZE[mode],
  });
  $("models-status").textContent = "";
  $("models-progress").hidden = true;
  $<HTMLButtonElement>("models-start").disabled = false;
  $<HTMLDialogElement>("models-dialog").showModal();
}

$("models-start").addEventListener("click", async () => {
  const start = $<HTMLButtonElement>("models-start");
  const bar = $<HTMLProgressElement>("models-progress");
  start.disabled = true;
  bar.hidden = false;
  bar.removeAttribute("value");
  const stop = await api.onProgress((p) => {
    if (p.total) {
      bar.max = p.total;
      bar.value = p.downloaded;
    }
    $("models-status").textContent = `${t("正在下载 {model}", { model: p.model })} · ${(p.downloaded / 1048576).toFixed(0)} MB`;
  });
  try {
    applyModels(await api.downloadModels(downloadMode));
    $("models-status").textContent = t("模型已就绪");
    toast(t("模型已就绪"));
    $<HTMLDialogElement>("models-dialog").close();
  } catch (e) {
    $("models-status").textContent = t("下载失败：{error}", { error: errorCode(e).detail });
    api.listModels().then(applyModels).catch(() => undefined);
    start.disabled = false;
  } finally {
    stop();
    renderStatus();
  }
});

// ------------------------------------------------------------------ export

function buildExportMenu(): void {
  const menu = $("export-menu");
  const items: HTMLElement[] = [];
  for (const f of EXPORT_FORMATS) {
    const label = f.needsPandoc && !snap.pandoc ? `${f.label} · Pandoc` : f.label;
    items.push(menuButton(t("导出为 {format}", { format: label }), () => exportAs(editor.value, f.id, f.ext, f.label), f.needsPandoc && !snap.pandoc));
  }
  const hr = document.createElement("hr");
  const group = document.createElement("div");
  group.className = "group";
  group.textContent = t("复制为");
  items.push(hr, group);
  for (const c of COPY_FORMATS) items.push(menuButton(c.label, () => copyAs(editor.value, c.id, c.label)));
  menu.replaceChildren(...items);
}

$("export-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  const menu = $("export-menu");
  const show = menu.hidden;
  closeMenus();
  menu.hidden = !show;
});

async function copyAs(content: string, format: CopyFormat, label: string): Promise<void> {
  if (!content.trim()) return toast(t("内容为空"), true);
  try {
    const text = format === "mathml" ? await toMathML(content) : wrapLatex(content, format);
    await copyText(text, label);
  } catch (e) {
    toast(t("导出失败：{error}", { error: String((e as Error).message ?? e) }), true);
  }
}

async function exportPayload(content: string, format: string): Promise<string> {
  switch (format) {
    case "tex":
      return toTexDocument(content);
    case "md":
    case "docx":
    case "odt":
      return toMarkdown(content);
    case "html":
      return toHtmlDocument(content);
    case "mml":
      return toMathML(content);
    case "svg":
      return toSvg(content);
    default:
      return content.trim() + "\n";
  }
}

async function exportAs(content: string, format: string, ext: string, label: string): Promise<void> {
  if (!content.trim()) return toast(t("内容为空"), true);
  try {
    const path = await api.saveDialog(`latexsnipper.${ext}`, ext, label);
    if (!path) return;
    await api.exportFile(path, await exportPayload(content, format), format);
    toast(t("已导出到 {path}", { path }));
  } catch (e) {
    toast(t("导出失败：{error}", { error: errorCode(e).detail }), true);
  }
}

// ------------------------------------------------------------------ model files

const MODEL_NAMES: [string, string][] = [
  ["mathcraft-formula-det", "公式检测"],
  ["mathcraft-formula-rec", "公式识别"],
  ["mathcraft-text-det", "文字检测"],
  ["mathcraft-text-rec", "文字识别"],
];
const modelName = (id: string) => t(MODEL_NAMES.find(([key]) => key === id)?.[1] ?? id);

type RowState = { downloading: boolean; deleting: boolean; done: number; total: number; failed: string | null };
const rowStates = new Map<string, RowState>();
/** Single-model downloads this window started and is still awaiting. */
const pendingDownloads = new Set<string>();

function rowState(m: ModelInfo): RowState {
  let s = rowStates.get(m.id);
  if (!s) rowStates.set(m.id, (s = { downloading: m.downloading, deleting: false, done: 0, total: m.download_bytes, failed: null }));
  return s;
}

function applyModels(state: ModelsState): void {
  snap.models = state.modes;
  snap.model_files = state.models;
  // The backend is the source of truth for what is installed and in flight.
  for (const m of state.models) {
    const s = rowStates.get(m.id);
    if (s) s.downloading = pendingDownloads.has(m.id) || m.downloading;
  }
  renderStatus();
  renderModelList();
}

function renderModelRow(li: HTMLLIElement, m: ModelInfo): void {
  const s = rowState(m);
  const pct = percentOf(s.done, s.total);
  const stateText = s.downloading
    ? pct === null
      ? t("下载中…")
      : t("下载中 {percent}%", { percent: pct })
    : s.failed
      ? t("下载失败")
      : t(m.installed ? "已下载" : "未下载");
  li.className = `model-row ${s.downloading ? "is-downloading" : s.failed ? "is-failed" : m.installed ? "is-installed" : "is-missing"}`;
  li.querySelector(".model-state")!.textContent = stateText;
  li.querySelector<HTMLElement>(".model-state")!.title = s.failed ?? "";
  const btn = li.querySelector<HTMLButtonElement>(".model-download")!;
  btn.textContent = t(m.installed ? "重新下载" : "下载");
  btn.className = `model-download ${m.installed ? "btn quiet small" : "btn small"}`;
  btn.disabled = s.downloading || s.deleting;
  // Delete only exists for a model that is on disk.
  const del = li.querySelector<HTMLButtonElement>(".model-delete")!;
  del.textContent = t("删除");
  del.hidden = !m.installed;
  del.disabled = s.downloading || s.deleting;
  const bar = li.querySelector<HTMLProgressElement>("progress")!;
  bar.hidden = !s.downloading;
  if (pct === null) bar.removeAttribute("value");
  else bar.value = pct;
}

function renderModelList(): void {
  const list = $("model-list");
  const models = snap.model_files ?? [];
  $("models-count").textContent = t("已下载 {n}/{total}", {
    n: models.filter((m) => m.installed).length,
    total: models.length,
  });
  list.replaceChildren(
    ...models.map((m) => {
      const li = document.createElement("li");
      li.dataset.model = m.id;
      li.title = m.id;
      const uses = m.used_by.map(modeLabel).join(getLang() === "zh" ? "、" : ", ");
      li.innerHTML = `<div><span class="model-name"></span><span class="model-use"></span></div>
        <span class="model-size"></span><span class="model-state" aria-live="polite"></span>
        <div class="model-actions"><button type="button" class="model-download"></button><button type="button" class="model-delete btn quiet small danger"></button></div>
        <progress max="100"></progress>`;
      li.querySelector(".model-name")!.textContent = modelName(m.id);
      li.querySelector(".model-use")!.textContent = t("用于：{modes}", { modes: uses });
      li.querySelector(".model-size")!.textContent = formatBytes(m.download_bytes);
      li.querySelector(".model-download")!.addEventListener("click", () => downloadOne(m.id));
      li.querySelector(".model-delete")!.addEventListener("click", () => deleteOne(m.id));
      renderModelRow(li, m);
      return li;
    }),
  );
}

function refreshModelRow(id: string): void {
  const m = snap.model_files?.find((x) => x.id === id);
  const li = $("model-list").querySelector<HTMLLIElement>(`li[data-model="${id}"]`);
  if (m && li) renderModelRow(li, m);
}

async function downloadOne(id: string): Promise<void> {
  const m = snap.model_files.find((x) => x.id === id);
  if (!m) return;
  const s = rowState(m);
  if (s.downloading) return;
  Object.assign(s, { downloading: true, done: 0, total: m.download_bytes, failed: null });
  pendingDownloads.add(id);
  refreshModelRow(id);
  try {
    const state = await api.downloadModel(id);
    pendingDownloads.delete(id);
    applyModels(state);
    toast(t("{name} 已下载", { name: modelName(id) }));
  } catch (e) {
    pendingDownloads.delete(id);
    const { code, detail } = errorCode(e);
    s.downloading = code === "DOWNLOAD_IN_PROGRESS"; // already running elsewhere: keep showing it
    s.failed = code === "DOWNLOAD_IN_PROGRESS" ? null : detail;
    refreshModelRow(id);
    toast(code === "DOWNLOAD_IN_PROGRESS" ? t("该模型正在下载") : t("下载失败：{error}", { error: detail }), code !== "DOWNLOAD_IN_PROGRESS");
  }
}

async function deleteOne(id: string): Promise<void> {
  const m = snap.model_files.find((x) => x.id === id);
  if (!m || !m.installed) return;
  const s = rowState(m);
  if (s.downloading || s.deleting) return;
  const modes = m.used_by.map(modeLabel).join(getLang() === "zh" ? "、" : ", ");
  const question = t("删除“{name}”模型？{modes}识别将无法使用，需要时可重新下载（{size}）。", {
    name: modelName(id),
    modes,
    size: formatBytes(m.download_bytes),
  });
  if ((await ask(question, t("删除"), undefined, true)) === null) return;
  s.deleting = true;
  s.failed = null;
  refreshModelRow(id);
  try {
    applyModels(await api.deleteModel(id));
    toast(t("{name} 已删除", { name: modelName(id) }));
  } catch (e) {
    const { code, detail } = errorCode(e);
    toast(code === "DOWNLOAD_IN_PROGRESS" ? t("该模型正在下载") : t("删除失败：{error}", { error: detail }), true);
  } finally {
    s.deleting = false;
    refreshModelRow(id);
  }
}

// ------------------------------------------------------------------ settings

const form = $<HTMLFormElement>("settings-form");
const field = <T extends HTMLElement>(name: string) => form.elements.namedItem(name) as unknown as T;

/** Show the fields for the backend that is selected right now (before saving). */
function syncBackendVisibility(): void {
  const external = (form.querySelector<HTMLInputElement>("input[name=backend]:checked")?.value ?? "builtin") === "external";
  $("external-fields").hidden = !external;
  $("builtin-fields").hidden = external;
}

function readSettingsForm(): Settings {
  const fd = new FormData(form);
  return {
    ...snap.settings,
    mode: fd.get("mode") as Mode,
    backend: fd.get("backend") as Settings["backend"],
    theme: fd.get("theme") as Settings["theme"],
    language: fd.get("language") as Settings["language"],
    pdf_dpi: Number(fd.get("pdf_dpi")) || 200,
    external_base_url: String(fd.get("external_base_url") ?? "").trim(),
    external_model: String(fd.get("external_model") ?? "").trim(),
    external_api_key: String(fd.get("external_api_key") ?? "").trim(),
    external_timeout_sec: Number(fd.get("external_timeout_sec")) || 60,
    external_custom_prompt: String(fd.get("external_custom_prompt") ?? ""),
  };
}

function openSettings(): void {
  const dlg = $<HTMLDialogElement>("settings-dialog");
  if (dlg.open) return;
  if (tour.isOpen) tour.end(false);
  document.querySelectorAll<HTMLDialogElement>("dialog[open]").forEach((d) => d.close());
  const s = snap.settings;
  form.querySelectorAll<HTMLInputElement>("input[type=radio]").forEach((r) => {
    r.checked = (s as unknown as Record<string, unknown>)[r.name] === r.value;
  });
  for (const k of ["language", "pdf_dpi", "external_base_url", "external_model", "external_api_key", "external_timeout_sec", "external_custom_prompt"] as const) {
    field<HTMLInputElement>(k).value = String(s[k]);
  }
  $<HTMLInputElement>("data-dir").value = snap.data_dir;
  $("version").textContent = `LaTeXSnipper ${snap.version}`;
  $("test-result").textContent = "";
  renderModelList();
  // Pick up files added or removed outside the app since the last look.
  api.listModels().then(applyModels).catch(() => undefined);
  syncBackendVisibility();
  dlg.returnValue = "";
  dlg.showModal();
}

$("settings-btn").addEventListener("click", openSettings);

form.addEventListener("change", (e) => {
  syncBackendVisibility();
  // Preview the theme on click; Save keeps it, Cancel/Esc restores the saved one.
  const target = e.target as HTMLInputElement;
  if (target.name === "theme" && target.checked) applyTheme(target.value as Settings["theme"]);
});

$("test-external").addEventListener("click", async () => {
  const out = $("test-result");
  out.textContent = "…";
  try {
    out.textContent = t("连接成功：{message}", { message: await api.testExternal(readSettingsForm()) });
  } catch (e) {
    out.textContent = t("连接失败：{error}", { error: errorCode(e).detail });
  }
});

$<HTMLDialogElement>("settings-dialog").addEventListener("close", async () => {
  if ($<HTMLDialogElement>("settings-dialog").returnValue !== "save") {
    applyTheme(snap.settings.theme);
    return;
  }
  try {
    snap.settings = await api.saveSettings(readSettingsForm());
    applyTheme(snap.settings.theme);
    applyLanguage();
    toast(t("设置已保存"));
  } catch (e) {
    applyTheme(snap.settings.theme);
    toast(errorCode(e).detail, true);
  }
});


// ------------------------------------------------------------------ hotkey

let draftHotkey = DEFAULT_HOTKEY;
const keycap = $("hotkey-capture");

function showDraft(): void {
  keycap.textContent = formatAccelerator(draftHotkey);
}

const hotkeyDialog = $<HTMLDialogElement>("hotkey-dialog");

$("hotkey-btn").addEventListener("click", () => {
  draftHotkey = snap.settings.hotkey;
  showDraft();
  $("hotkey-hint").textContent = t("按下新的组合键…");
  hotkeyDialog.returnValue = "";
  // While recording, the registered global shortcut must not swallow the keys.
  api.suspendHotkey().catch(() => undefined);
  hotkeyDialog.showModal();
  keycap.classList.add("listening");
  keycap.focus();
});

// Listen on the whole dialog: WebKit does not focus a <button> when it is
// clicked, so a listener on the keycap alone goes deaf after any click.
hotkeyDialog.addEventListener(
  "keydown",
  (e) => {
    const modified = e.metaKey || e.ctrlKey || e.altKey;
    if (!modified && ["Tab", "Escape", "Enter", " "].includes(e.key)) return;
    e.preventDefault();
    e.stopPropagation();
    if (["Meta", "Control", "Alt", "Shift", "CapsLock", "Fn"].includes(e.key)) return;
    const acc = acceleratorFromEvent(e);
    if (!acc) {
      $("hotkey-hint").textContent = t("组合键需要至少一个修饰键（⌘ ⌥ ⌃ ⇧）");
      return;
    }
    draftHotkey = acc;
    $("hotkey-hint").textContent = "";
    showDraft();
  },
  true,
);

keycap.addEventListener("click", () => keycap.classList.add("listening"));

$("hotkey-reset").addEventListener("click", () => {
  draftHotkey = DEFAULT_HOTKEY;
  showDraft();
});

hotkeyDialog.addEventListener("close", async () => {
  keycap.classList.remove("listening");
  try {
    if (hotkeyDialog.returnValue !== "save" || draftHotkey === snap.settings.hotkey) return;
    snap.settings = await api.saveSettings({ ...snap.settings, hotkey: draftHotkey });
    $("capture-kbd").textContent = formatAccelerator(draftHotkey);
    renderHistory();
    toast(t("快捷键已更新为 {hotkey}", { hotkey: formatAccelerator(draftHotkey) }));
  } catch (e) {
    toast(t("快捷键不可用：{error}", { error: errorCode(e).detail }), true);
  } finally {
    await api.resumeHotkey().catch(() => undefined);
  }
});

// ------------------------------------------------------------------ favorites

$("favorites-btn").addEventListener("click", () => {
  renderFavorites();
  $<HTMLDialogElement>("favorites-dialog").showModal();
});

$("favorites-clear").addEventListener("click", async () => {
  if (!snap.favorites.length) return;
  if ((await ask(t("确定要清空全部 {count} 条收藏吗？", { count: snap.favorites.length }), t("清空全部"))) === null) return;
  snap.favorites = await api.favorites({ op: "clear" });
  renderFavorites();
});

$("favorites-export").addEventListener("click", async () => {
  const md = snap.favorites
    .map((f, i) => `## ${f.name || `#${i + 1}`}\n\n${f.kind === "mathcraft" ? `$$\n${f.text}\n$$` : f.text}`)
    .join("\n\n");
  await exportAs(md, "md", "md", "Markdown");
});

// ------------------------------------------------------------------ handwriting

const ink = new InkModel();
const canvas = $<HTMLCanvasElement>("hand-canvas");
const handText = $<HTMLTextAreaElement>("hand-text");
let handSeq = 0;

function redrawInk(): void {
  const dpr = window.devicePixelRatio || 1;
  const { width, height } = canvas.getBoundingClientRect();
  if (canvas.width !== Math.round(width * dpr)) {
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
  }
  const ctx = canvas.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  drawStrokes(ctx, ink.strokes, getComputedStyle(document.documentElement).getPropertyValue("--text").trim() || "#000");
  $("hand-hint").hidden = !ink.isEmpty();
}

const recognizeInk = debounce(async () => {
  const png = inkToPng(ink);
  if (!png) return;
  const seq = ++handSeq;
  $("hand-status").textContent = t("识别中…");
  try {
    const r = await api.recognizeImage(png, "formula");
    if (seq !== handSeq) return;
    handText.value = r.text;
    renderPreview($("hand-preview"), r.text);
    $("hand-status").textContent = "";
  } catch (e) {
    if (seq !== handSeq) return;
    const { code, detail } = errorCode(e);
    $("hand-status").textContent = code === "EMPTY_RESULT" ? t("未检测到可识别内容") : detail;
    if (code === "MODELS_MISSING") openModels("formula");
  }
}, 900);

function inkChanged(): void {
  redrawInk();
  if (ink.isEmpty()) {
    handSeq++;
    recognizeInk.cancel();
    handText.value = "";
    $("hand-preview").replaceChildren();
  } else recognizeInk();
}

let drawing = false;
const tool = () => document.querySelector<HTMLInputElement>("input[name=tool]:checked")?.value ?? "pen";
const point = (e: PointerEvent): [number, number] => {
  const r = canvas.getBoundingClientRect();
  return [e.clientX - r.left, e.clientY - r.top];
};
canvas.addEventListener("pointerdown", (e) => {
  canvas.setPointerCapture(e.pointerId);
  drawing = true;
  recognizeInk.cancel();
  if (tool() === "pen") ink.begin(point(e), e.pointerType === "pen" ? 2 + e.pressure * 2.5 : 3.2);
  else ink.erase(point(e));
  redrawInk();
});
canvas.addEventListener("pointermove", (e) => {
  if (!drawing) return;
  if (tool() === "pen") ink.extend(point(e));
  else ink.erase(point(e), 10, false);
  redrawInk();
});
canvas.addEventListener("pointerup", () => {
  if (!drawing) return;
  drawing = false;
  inkChanged();
});
$("hand-undo").addEventListener("click", () => ink.undo() && inkChanged());
$("hand-clear").addEventListener("click", () => {
  ink.clear();
  inkChanged();
});
handText.addEventListener("input", debounce(() => renderPreview($("hand-preview"), handText.value), 120));
$("hand-copy").addEventListener("click", () => copyText(handText.value));
$("hand-insert").addEventListener("click", () => {
  const text = handText.value.trim();
  if (!text) return toast(t("内容为空"), true);
  const pos = editor.selectionStart ?? editor.value.length;
  editor.setRangeText(text, pos, editor.selectionEnd ?? pos, "end");
  refreshPreview();
  toast(t("已插入编辑器"));
});
$("handwriting-btn").addEventListener("click", () => {
  $<HTMLDialogElement>("hand-dialog").showModal();
  requestAnimationFrame(redrawInk);
});
new ResizeObserver(() => redrawInk()).observe(canvas);

// ------------------------------------------------------------------ pdf

const pdfInput = $<HTMLInputElement>("pdf-input");
let pdfBytes: ArrayBuffer | null = null;
let pdfPageCount = 0;
let pdfAbort = false;

$("pdf-btn").addEventListener("click", () => pdfInput.click());
pdfInput.addEventListener("change", async () => {
  const f = pdfInput.files?.[0];
  pdfInput.value = "";
  if (!f) return;
  try {
    pdfBytes = await f.arrayBuffer();
    const { openPdf } = await import("./lib/pdf");
    const doc = await openPdf(pdfBytes.slice(0));
    pdfPageCount = doc.numPages;
    await doc.destroy();
  } catch (e) {
    return toast(t("无法打开 PDF：{error}", { error: String((e as Error).message ?? e) }), true);
  }
  $("pdf-title").textContent = `${t("PDF 选项")} · ${f.name}`;
  $("pdf-pages").textContent = t("共 {count} 页", { count: pdfPageCount });
  $<HTMLInputElement>("pdf-range").value = "";
  $<HTMLInputElement>("pdf-dpi").value = String(snap.settings.pdf_dpi);
  $("pdf-options").hidden = false;
  $("pdf-run").hidden = true;
  for (const [id, on] of [["pdf-start", true], ["pdf-insert", false], ["pdf-export", false]] as const) $(id).hidden = !on;
  $("pdf-cancel").textContent = t("取消");
  $<HTMLDialogElement>("pdf-dialog").showModal();
});

$("pdf-start").addEventListener("click", async () => {
  const pages = parsePageRange($<HTMLInputElement>("pdf-range").value, pdfPageCount);
  if (!pages) return toast(t("页码范围无效"), true);
  const dpi = Math.min(600, Math.max(72, Number($<HTMLInputElement>("pdf-dpi").value) || 200));
  $("pdf-options").hidden = true;
  $("pdf-run").hidden = false;
  $("pdf-start").hidden = true;
  $("pdf-cancel").textContent = t("停止");
  const bar = $<HTMLProgressElement>("pdf-progress");
  const out = $<HTMLTextAreaElement>("pdf-text");
  out.value = "";
  bar.max = pages.length;
  bar.value = 0;
  pdfAbort = false;
  const results: { page: number; text: string }[] = [];
  setBusy(true);
  try {
    const { openPdf, renderPage } = await import("./lib/pdf");
    const doc = await openPdf(pdfBytes!.slice(0));
    for (const [i, page] of pages.entries()) {
      if (pdfAbort) break;
      $("pdf-status").textContent = t("正在识别第 {page}/{total} 页", { page: i + 1, total: pages.length });
      const png = await renderPage(doc, page, dpi);
      try {
        const r = await api.recognizeImage(png, "mixed");
        results.push({ page, text: r.text });
      } catch (e) {
        const { code } = errorCode(e);
        if (code === "MODELS_MISSING") {
          $<HTMLDialogElement>("pdf-dialog").close();
          await doc.destroy();
          return openModels("mixed");
        }
        if (code !== "EMPTY_RESULT") throw e;
      }
      out.value = joinPdfPages(results);
      bar.value = i + 1;
    }
    await doc.destroy();
    $("pdf-status").textContent = pdfAbort ? t("已停止") : t("PDF 识别结果");
  } catch (e) {
    $("pdf-status").textContent = t("识别失败：{error}", { error: errorCode(e).detail });
  } finally {
    setBusy(false);
    $("pdf-cancel").textContent = t("关闭");
    $("pdf-insert").hidden = false;
    $("pdf-export").hidden = false;
  }
});

$<HTMLDialogElement>("pdf-dialog").addEventListener("cancel", () => (pdfAbort = true));
$("pdf-cancel").addEventListener("click", () => (pdfAbort = true));
$("pdf-insert").addEventListener("click", () => {
  const text = $<HTMLTextAreaElement>("pdf-text").value;
  loadIntoEditor(text, "mathcraft_mixed");
  $<HTMLDialogElement>("pdf-dialog").close();
});
$("pdf-export").addEventListener("click", () => exportAs($<HTMLTextAreaElement>("pdf-text").value, "md", "md", "Markdown"));

// ------------------------------------------------------------------ quick start (spotlight tour)

const TOUR_STEPS: TourStep[] = [
  {
    targets: ["#capture-btn"],
    title: "截图识别",
    body: "按 {hotkey} 或点这里，框选屏幕上的公式。第一次使用时，macOS 会请求屏幕录制权限。",
  },
  {
    targets: ["#image-btn", "#pdf-btn", "#handwriting-btn"],
    title: "图片、PDF 与手写",
    body: "也可以识别图片文件、整份 PDF，或直接手写公式。把图片拖进窗口，或在窗口里按 {paste} 粘贴也行。",
  },
  {
    targets: ["#editor", "#preview-paper"],
    title: "编辑与预览",
    body: "识别结果会先在确认窗口里让你检查和修改，确认后填入这里。下方的方格纸实时渲染公式。",
  },
  {
    targets: ["#copy-btn", "#export-btn"],
    title: "复制与导出",
    body: "复制编辑器里的内容，或导出为 TeX、Markdown、SVG 等格式。导出菜单里还能复制为 $…$、equation 或 MathML。",
  },
  {
    targets: ["#history"],
    title: "历史记录",
    body: "每次确认的结果都记在这里。点一条载入编辑器，右键可以重命名、收藏或删除。",
  },
  {
    targets: [".side-tools"],
    title: "快捷键与设置",
    body: "在这里更改截图快捷键、打开收藏夹。识别类型、模型下载、主题和语言都在“设置”里。",
  },
  {
    targets: ["#onboarding-btn"],
    title: "后台运行",
    body: "关闭窗口后应用仍在后台运行，用 {hotkey} 或菜单栏的 Σ 图标随时截图，按 ⌘Q 完全退出。想再看一遍引导，点这个按钮。",
  },
];

/**
 * Builds without native capture (Windows, Linux) skip the capture step and get
 * wording without a capture shortcut or the macOS menu bar.
 */
function tourSteps(): TourStep[] {
  if (snap.capture_supported) return TOUR_STEPS;
  return TOUR_STEPS.filter((s) => !s.targets.includes("#capture-btn")).map((s) => {
    if (s.targets.includes(".side-tools")) {
      return { ...s, title: "收藏夹与设置", body: "在这里打开收藏夹、清空历史。识别类型、模型下载、主题和语言都在“设置”里。" };
    }
    if (s.targets.includes("#onboarding-btn")) {
      return {
        ...s,
        body: "关闭窗口后应用仍在后台运行，可以从系统托盘图标重新打开或退出。想再看一遍引导，点这个按钮。",
      };
    }
    return s;
  });
}

const isMac = /Mac/.test(navigator.platform);

const tour = new SpotlightTour(
  tourSteps,
  () => {
    const hotkey = formatAccelerator(snap.settings.hotkey);
    return {
      title: (s) => t(s.title),
      body: (s) => t(s.body, { hotkey, paste: isMac ? "⌘V" : "Ctrl+V" }),
      skip: t("跳过"),
      back: t("上一步"),
      next: t("下一步"),
      done: t("开始使用"),
    };
  },
  async () => {
    // Finishing or skipping both count: the tour is offered once, then lives behind the ? button.
    if (!snap.settings.onboarding_done) snap.settings = await api.saveSettings({ ...snap.settings, onboarding_done: true });
  },
);

function openTour(): void {
  closeMenus();
  document.querySelectorAll<HTMLDialogElement>("dialog[open]").forEach((d) => d.close());
  tour.start();
}

$("onboarding-btn").addEventListener("click", openTour);

// ------------------------------------------------------------------ boot

/** Headless smoke test: prove the bundled UI, MathJax and IPC work, then let the app exit. */
async function selfcheck(): Promise<void> {
  try {
    const probe = document.createElement("div");
    await renderInto(probe, "\\frac{a+b}{c^{2}}");
    const ok = !!probe.querySelector("mjx-container svg");
    await api.frontendReady(ok, ok ? "mathjax rendered" : "mathjax produced no svg");
  } catch (e) {
    await api.frontendReady(false, String((e as Error).message ?? e));
  }
}

async function boot(): Promise<void> {
  snap = await api.bootstrap();
  $("capture-btn").hidden = !snap.capture_supported;
  $("hotkey-btn").hidden = !snap.capture_supported;
  applyTheme(snap.settings.theme);
  applyLanguage();
  const last = snap.history[snap.history.length - 1];
  if (last) loadIntoEditor(last.text, last.kind);
  if (snap.capture_supported) await api.onHotkey(capture);
  await api.onOpenSettings(openSettings);
  await api.onProgress((p) => {
    const s = rowStates.get(p.model);
    if (!s) return;
    s.downloading = true;
    s.done = p.downloaded;
    if (p.total) s.total = p.total;
    refreshModelRow(p.model);
  });
  await api
    .onDrop(
      (paths) => {
        const p = paths.find((x) => /\.(png|jpe?g|bmp|webp|gif|tiff?)$/i.test(x));
        if (p) recognizeWith(() => api.recognizeFile(p, snap.settings.mode), snap.settings.mode);
      },
      (on) => ($("drop-veil").hidden = !on),
    )
    .catch(() => undefined);
  if (!snap.settings.onboarding_done) openTour();
  document.body.dataset.ready = "true";
  if (snap.selfcheck) await selfcheck();
}

boot().catch((e) => toast(String(e), true));

