// Pure helpers: content classification, export payloads, hotkeys, PDF ranges.

export type Mode = "formula" | "mixed" | "text";
export type Kind = "mathcraft" | "mathcraft_mixed" | "mathcraft_text";

export interface Entry {
  text: string;
  name: string;
  kind: Kind;
}

export const kindForMode = (mode: Mode): Kind =>
  mode === "formula" ? "mathcraft" : mode === "mixed" ? "mathcraft_mixed" : "mathcraft_text";

/** Display order with original indices (history numbering stays stable). */
export function orderedEntries(entries: Entry[], newestFirst: boolean): { entry: Entry; index: number }[] {
  const indexed = entries.map((entry, index) => ({ entry, index }));
  return newestFirst ? indexed.reverse() : indexed;
}

/** Treat content as Markdown-with-math when it carries $ delimiters or prose lines. */
export function isMixedContent(text: string): boolean {
  return /(^|[^\\])\$/.test(text);
}

export type Segment = { type: "text"; value: string } | { type: "math"; value: string; display: boolean };

/** Split Markdown-with-math into text and `$…$` / `$$…$$` segments. */
export function splitMath(source: string): Segment[] {
  const out: Segment[] = [];
  let i = 0;
  let buf = "";
  const flush = () => {
    if (buf) out.push({ type: "text", value: buf });
    buf = "";
  };
  while (i < source.length) {
    const ch = source[i];
    if (ch === "\\" && source[i + 1] === "$") {
      buf += "$";
      i += 2;
      continue;
    }
    if (ch === "$") {
      const display = source[i + 1] === "$";
      const open = display ? 2 : 1;
      const close = source.indexOf(display ? "$$" : "$", i + open);
      if (close > i + open - 1 && (display || !source.slice(i + open, close).includes("\n\n"))) {
        flush();
        out.push({ type: "math", value: source.slice(i + open, close).trim(), display });
        i = close + open;
        continue;
      }
    }
    buf += ch;
    i += 1;
  }
  flush();
  return out;
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

export interface ExportFormat {
  id: string;
  ext: string;
  label: string;
  needsPandoc?: boolean;
}

export const EXPORT_FORMATS: ExportFormat[] = [
  { id: "tex", ext: "tex", label: "LaTeX (.tex)" },
  { id: "md", ext: "md", label: "Markdown (.md)" },
  { id: "html", ext: "html", label: "HTML (.html)" },
  { id: "mml", ext: "mml", label: "MathML (.mml)" },
  { id: "svg", ext: "svg", label: "SVG (.svg)" },
  { id: "txt", ext: "txt", label: "Text (.txt)" },
  { id: "docx", ext: "docx", label: "Word (.docx)", needsPandoc: true },
  { id: "odt", ext: "odt", label: "OpenDocument (.odt)", needsPandoc: true },
];

export const COPY_FORMATS = [
  { id: "raw", label: "LaTeX" },
  { id: "inline", label: "$…$" },
  { id: "display", label: "$$…$$" },
  { id: "bracket", label: "\\[…\\]" },
  { id: "equation", label: "equation" },
  { id: "mathml", label: "MathML" },
] as const;

export type CopyFormat = (typeof COPY_FORMATS)[number]["id"];

export function wrapLatex(latex: string, format: Exclude<CopyFormat, "mathml">): string {
  const body = latex.trim();
  switch (format) {
    case "raw":
      return body;
    case "inline":
      return `$${body}$`;
    case "display":
      return `$$\n${body}\n$$`;
    case "bracket":
      return `\\[\n${body}\n\\]`;
    case "equation":
      return `\\begin{equation}\n${body}\n\\end{equation}`;
  }
}

/** Standalone LaTeX document; formulas get a display environment, Markdown stays as text. */
export function toTexDocument(content: string): string {
  const body = isMixedContent(content)
    ? splitMath(content)
        .map((s) => (s.type === "text" ? s.value : s.display ? `\\[\n${s.value}\n\\]` : `$${s.value}$`))
        .join("")
    : `\\[\n${content.trim()}\n\\]`;
  return [
    "\\documentclass{article}",
    "\\usepackage{amsmath,amssymb}",
    "\\usepackage[UTF8]{ctex}",
    "\\begin{document}",
    body,
    "\\end{document}",
    "",
  ].join("\n");
}

export function toMarkdown(content: string): string {
  return isMixedContent(content) ? content.trim() + "\n" : `$$\n${content.trim()}\n$$\n`;
}

// ---------------------------------------------------------------- hotkeys

const MODIFIER_ORDER = ["CommandOrControl", "Control", "Alt", "Shift"] as const;
const SYMBOLS: Record<string, string> = { CommandOrControl: "⌘", Control: "⌃", Alt: "⌥", Shift: "⇧" };

/** Build a Tauri accelerator from a keydown event; null when it isn't a usable chord. */
export function acceleratorFromEvent(e: Pick<KeyboardEvent, "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "code">): string | null {
  const mods: string[] = [];
  if (e.metaKey) mods.push("CommandOrControl");
  if (e.ctrlKey) mods.push("Control");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  let key: string | null = null;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
  else if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
  else if (/^F([1-9]|1[0-2])$/.test(e.code)) key = e.code;
  else if (e.code === "Space") key = "Space";
  if (!key || mods.length === 0) return null;
  if (mods.length === 1 && mods[0] === "Shift") return null;
  return [...mods, key].join("+");
}

export function formatAccelerator(acc: string): string {
  const parts = acc.split("+");
  const key = parts.pop() ?? "";
  const mods = MODIFIER_ORDER.filter((m) => parts.includes(m) || (m === "CommandOrControl" && parts.includes("Command")));
  // macOS menu order: ⌃ ⌥ ⇧ ⌘
  const mac = ["Control", "Alt", "Shift", "CommandOrControl"].filter((m) => mods.includes(m as never));
  return mac.map((m) => SYMBOLS[m]).join("") + key;
}

// ---------------------------------------------------------------- pdf

/** Parse "1-3,5" into sorted unique 1-based page numbers; null when invalid. */
export function parsePageRange(input: string, total: number): number[] | null {
  const text = input.replace(/\s+/g, "");
  if (!text) return Array.from({ length: total }, (_, i) => i + 1);
  const pages = new Set<number>();
  for (const part of text.split(/[,，]/)) {
    const m = /^(\d+)(?:-(\d+))?$/.exec(part);
    if (!m) return null;
    const a = Number(m[1]);
    const b = m[2] ? Number(m[2]) : a;
    if (a < 1 || b < a || b > total) return null;
    for (let p = a; p <= b; p++) pages.add(p);
  }
  return [...pages].sort((x, y) => x - y);
}

export function joinPdfPages(pages: { page: number; text: string }[]): string {
  return pages
    .filter((p) => p.text.trim())
    .map((p) => `<!-- page ${p.page} -->\n\n${p.text.trim()}`)
    .join("\n\n");
}

export function debounce<A extends unknown[]>(fn: (...a: A) => void, ms: number): ((...a: A) => void) & { cancel(): void } {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const wrapped = (...a: A) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...a), ms);
  };
  wrapped.cancel = () => clearTimeout(timer);
  return wrapped;
}

/** Stable error codes from the backend → user-facing message keys. */
export function errorCode(err: unknown): {
  code: "MODELS_MISSING" | "EMPTY_RESULT" | "HOTKEY_INVALID" | "DOWNLOAD_IN_PROGRESS" | "OTHER";
  detail: string;
} {
  const msg = String((err as { message?: string })?.message ?? err);
  for (const code of ["MODELS_MISSING", "EMPTY_RESULT", "HOTKEY_INVALID", "DOWNLOAD_IN_PROGRESS"] as const) {
    if (msg.startsWith(code)) return { code, detail: msg.slice(code.length + 1) };
  }
  return { code: "OTHER", detail: msg };
}

export type ThemeSetting = "auto" | "light" | "dark";

/** The palette to paint for a setting; "auto" follows the system appearance. */
export function resolveTheme(theme: ThemeSetting, systemDark: boolean): "light" | "dark" {
  return theme === "auto" ? (systemDark ? "dark" : "light") : theme;
}

/** Native window appearance: null hands control back to the system. */
export function nativeTheme(theme: ThemeSetting): "light" | "dark" | null {
  return theme === "auto" ? null : theme;
}

/** Download sizes: one decimal below 10 MB, whole megabytes above. */
export function formatBytes(bytes: number): string {
  const mb = bytes / 1048576;
  if (mb < 1) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return mb < 10 ? `${mb.toFixed(1)} MB` : `${Math.round(mb)} MB`;
}

/** Whole-number percentage, or null when the total size is unknown. */
export function percentOf(done: number, total: number): number | null {
  if (!total) return null;
  return Math.min(100, Math.floor((done / total) * 100));
}
