// The result window: reviews a capture taken while the main window was hidden,
// so only this small window appears. The main window's own result dialog
// handles captures started from it.
import { api, type Recognition, type Settings } from "./lib/api";
import { debounce, nativeTheme, resolveTheme } from "./lib/format";
import { getLang, setLang, t, translateDom } from "./lib/i18n";
import { loadMathJax, renderInto } from "./lib/math";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

const form = $<HTMLFormElement>("result-form");
const resultText = $<HTMLTextAreaElement>("result-text");
const preview = $("result-preview");
const warn = $("result-warn");
const systemDark = matchMedia("(prefers-color-scheme: dark)");

let pending: Recognition | null = null;
let theme: Settings["theme"] = "auto";

const render = () => renderInto(preview, resultText.value).catch(() => undefined);
const refresh = debounce(render, 120);
resultText.addEventListener("input", refresh);

function applyLook(nextTheme: Settings["theme"], language: Settings["language"]): void {
  theme = nextTheme;
  document.documentElement.dataset.theme = resolveTheme(theme, systemDark.matches);
  api.setWindowTheme(nativeTheme(theme)).catch(() => undefined);
  setLang(language);
  document.documentElement.lang = getLang() === "zh" ? "zh-CN" : "en";
  translateDom(document);
}

systemDark.addEventListener("change", () => {
  document.documentElement.dataset.theme = resolveTheme(theme, systemDark.matches);
});

async function load(): Promise<void> {
  const view = await api.takeResult();
  if (!view) return;
  pending = view.result;
  applyLook(view.theme, view.language);
  resultText.value = pending.text;
  warn.hidden = pending.score >= 0.6 && !pending.flags.length;
  warn.textContent = t("低置信度，请检查结果") + (pending.flags.length ? ` (${pending.flags.join(", ")})` : "");
  refresh.cancel();
  await render();
  await api.presentResult();
  $("result-confirm").focus();
}

function dismiss(): void {
  pending = null;
  api.dismissResult().catch(() => undefined);
}

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  const text = resultText.value.trim();
  if (!pending || !text) return dismiss();
  try {
    await api.copy(text);
    const history = await api.history({ op: "add", text, kind: pending.kind });
    await api.resultConfirmed({ text, kind: pending.kind, history });
    dismiss();
  } catch (err) {
    warn.textContent = String((err as Error).message ?? err);
    warn.hidden = false;
  }
});

$("result-cancel").addEventListener("click", dismiss);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    dismiss();
  } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
    e.preventDefault();
    form.requestSubmit();
  }
});

loadMathJax().catch(() => undefined);
api.onResultReady(() => void load()).then(load).catch(() => undefined);
