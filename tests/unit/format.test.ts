import { describe, expect, it, vi } from "vitest";
import {
  acceleratorFromEvent,
  debounce,
  errorCode,
  formatAccelerator,
  isMixedContent,
  joinPdfPages,
  kindForMode,
  orderedEntries,
  parsePageRange,
  splitMath,
  toMarkdown,
  toTexDocument,
  wrapLatex,
  escapeHtml,
  nativeTheme,
  resolveTheme,
  formatBytes,
  percentOf,
  type Entry,
} from "../../frontend/lib/format";

const key = (code: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) => ({
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  code,
  ...mods,
});

describe("content classification", () => {
  it("detects Markdown-with-math only from unescaped dollars", () => {
    expect(isMixedContent("\\frac{a}{b}")).toBe(false);
    expect(isMixedContent("Let $x$ be")).toBe(true);
    expect(isMixedContent("costs \\$5")).toBe(false);
  });

  it("splits inline and display math", () => {
    expect(splitMath("a $x$ b $$y$$")).toEqual([
      { type: "text", value: "a " },
      { type: "math", value: "x", display: false },
      { type: "text", value: " b " },
      { type: "math", value: "y", display: true },
    ]);
  });

  it("keeps escaped and unterminated dollars as text", () => {
    expect(splitMath("\\$5 and $open")).toEqual([{ type: "text", value: "$5 and $open" }]);
    expect(splitMath("$a\n\nb$")).toEqual([{ type: "text", value: "$a\n\nb$" }]);
  });

  it("maps modes to history kinds", () => {
    expect(kindForMode("formula")).toBe("mathcraft");
    expect(kindForMode("mixed")).toBe("mathcraft_mixed");
    expect(kindForMode("text")).toBe("mathcraft_text");
  });

  it("escapes HTML", () => {
    expect(escapeHtml(`<a href="x">'&'</a>`)).toBe("&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
  });
});

describe("history ordering", () => {
  const entries: Entry[] = ["a", "b", "c"].map((text) => ({ text, name: "", kind: "mathcraft" }));
  it("keeps original indices when reversed", () => {
    expect(orderedEntries(entries, true).map((e) => [e.index, e.entry.text])).toEqual([
      [2, "c"],
      [1, "b"],
      [0, "a"],
    ]);
    expect(orderedEntries(entries, false)[0].index).toBe(0);
    expect(entries[0].text).toBe("a"); // input untouched
  });
});

describe("export payloads", () => {
  it("wraps LaTeX for copy formats", () => {
    expect(wrapLatex(" x ", "raw")).toBe("x");
    expect(wrapLatex("x", "inline")).toBe("$x$");
    expect(wrapLatex("x", "display")).toBe("$$\nx\n$$");
    expect(wrapLatex("x", "bracket")).toBe("\\[\nx\n\\]");
    expect(wrapLatex("x", "equation")).toBe("\\begin{equation}\nx\n\\end{equation}");
  });

  it("builds a compilable TeX document", () => {
    const formula = toTexDocument("a^2");
    expect(formula).toContain("\\documentclass{article}");
    expect(formula).toContain("\\[\na^2\n\\]");
    const mixed = toTexDocument("Let $x$ and $$y$$");
    expect(mixed).toContain("Let $x$ and \\[\ny\n\\]");
  });

  it("builds Markdown", () => {
    expect(toMarkdown("a")).toBe("$$\na\n$$\n");
    expect(toMarkdown("t $a$ ")).toBe("t $a$\n");
  });

  it("joins PDF pages and skips empty ones", () => {
    expect(joinPdfPages([{ page: 1, text: "A" }, { page: 2, text: " " }, { page: 3, text: "C\n" }])).toBe(
      "<!-- page 1 -->\n\nA\n\n<!-- page 3 -->\n\nC",
    );
  });
});

describe("page ranges", () => {
  it("parses ranges, lists and empty input", () => {
    expect(parsePageRange("", 3)).toEqual([1, 2, 3]);
    expect(parsePageRange("3, 1-2", 5)).toEqual([1, 2, 3]);
    expect(parsePageRange("2-4，4", 5)).toEqual([2, 3, 4]);
  });
  it("rejects invalid input", () => {
    for (const bad of ["0", "6", "3-1", "a", "1-", "1,,2"]) expect(parsePageRange(bad, 5)).toBeNull();
  });
});

describe("hotkeys", () => {
  it("builds accelerators from key events", () => {
    expect(acceleratorFromEvent(key("KeyF", { metaKey: true, altKey: true }))).toBe("CommandOrControl+Alt+F");
    expect(acceleratorFromEvent(key("Digit2", { ctrlKey: true, shiftKey: true }))).toBe("Control+Shift+2");
    expect(acceleratorFromEvent(key("F5", { altKey: true }))).toBe("Alt+F5");
  });
  it("rejects chords without a real modifier", () => {
    expect(acceleratorFromEvent(key("KeyA"))).toBeNull();
    expect(acceleratorFromEvent(key("KeyA", { shiftKey: true }))).toBeNull();
    expect(acceleratorFromEvent(key("Minus", { metaKey: true }))).toBeNull();
  });
  it("formats in macOS modifier order", () => {
    expect(formatAccelerator("CommandOrControl+Alt+F")).toBe("⌥⌘F");
    expect(formatAccelerator("Shift+Control+K")).toBe("⌃⇧K");
    expect(formatAccelerator("Command+Space")).toBe("⌘Space");
  });
});

describe("errors and timing", () => {
  it("extracts stable backend error codes", () => {
    expect(errorCode("MODELS_MISSING:a,b")).toEqual({ code: "MODELS_MISSING", detail: "a,b" });
    expect(errorCode(new Error("EMPTY_RESULT"))).toEqual({ code: "EMPTY_RESULT", detail: "" });
    expect(errorCode("boom")).toEqual({ code: "OTHER", detail: "boom" });
  });

  it("debounces and cancels", () => {
    vi.useFakeTimers();
    const fn = vi.fn();
    const d = debounce(fn, 100);
    d(1);
    d(2);
    vi.advanceTimersByTime(99);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledExactlyOnceWith(2);
    d(3);
    d.cancel();
    vi.advanceTimersByTime(200);
    expect(fn).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });
});

describe("themes", () => {
  it("resolves follow-system against the system appearance", () => {
    expect(resolveTheme("auto", true)).toBe("dark");
    expect(resolveTheme("auto", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });
  it("hands the native window back to the system only for follow-system", () => {
    expect(nativeTheme("auto")).toBeNull();
    expect(nativeTheme("light")).toBe("light");
    expect(nativeTheme("dark")).toBe("dark");
  });
});

describe("model downloads", () => {
  it("formats download sizes", () => {
    expect(formatBytes(67531517)).toBe("64 MB");
    expect(formatBytes(108795631)).toBe("104 MB");
    expect(formatBytes(4533780)).toBe("4.3 MB");
    expect(formatBytes(14102746)).toBe("13 MB");
    expect(formatBytes(2048)).toBe("2 KB");
  });
  it("computes progress, unknown when the size is missing", () => {
    expect(percentOf(50, 200)).toBe(25);
    expect(percentOf(199, 200)).toBe(99);
    expect(percentOf(300, 200)).toBe(100);
    expect(percentOf(10, 0)).toBeNull();
  });
  it("recognizes the in-progress error code", () => {
    expect(errorCode("DOWNLOAD_IN_PROGRESS:mathcraft-text-det")).toEqual({
      code: "DOWNLOAD_IN_PROGRESS",
      detail: "mathcraft-text-det",
    });
  });
});
