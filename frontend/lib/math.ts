// MathJax 3 (TeX → SVG, all extensions) loaded once from a bundled asset.
import mathjaxUrl from "mathjax/es5/tex-svg-full.js?url";
import { escapeHtml, isMixedContent, soleDisplayMath, splitMath } from "./format";

interface MathJaxApi {
  tex2svgPromise(tex: string, opts?: { display?: boolean }): Promise<HTMLElement>;
  tex2mmlPromise(tex: string, opts?: { display?: boolean }): Promise<string>;
  startup: { promise: Promise<void>; adaptor: { outerHTML(n: unknown): string } };
  texReset(): void;
  svgStylesheet(): HTMLStyleElement;
}

declare global {
  interface Window {
    MathJax: MathJaxApi | Record<string, unknown>;
  }
}

let ready: Promise<MathJaxApi> | null = null;

export function loadMathJax(): Promise<MathJaxApi> {
  if (ready) return ready;
  ready = new Promise((resolve, reject) => {
    window.MathJax = {
      tex: { inlineMath: [["$", "$"]], displayMath: [["$$", "$$"]], tags: "ams" },
      svg: { fontCache: "local" },
      options: { enableMenu: false },
      startup: { typeset: false },
    };
    const script = document.createElement("script");
    script.src = mathjaxUrl;
    script.async = true;
    script.onload = () => {
      const mj = window.MathJax as MathJaxApi;
      mj.startup.promise.then(() => {
        // tex2svg() doesn't install MathJax's CSS; without it the assistive MathML shows as a duplicate.
        document.head.appendChild(mj.svgStylesheet());
        resolve(mj);
      }, reject);
    };
    script.onerror = () => reject(new Error("MathJax failed to load"));
    document.head.appendChild(script);
  });
  return ready;
}

/** TeX parse errors render as merror nodes; surface them as exceptions instead. */
async function svgFor(mj: MathJaxApi, tex: string, display: boolean): Promise<HTMLElement> {
  mj.texReset();
  const node = await mj.tex2svgPromise(tex, { display });
  const err = node.querySelector("[data-mjx-error]");
  if (err) throw new Error(err.getAttribute("data-mjx-error") ?? "TeX error");
  return node;
}

/** Render LaTeX or Markdown-with-math into `target`. Throws on TeX errors. */
export async function renderInto(target: HTMLElement, source: string): Promise<void> {
  const mj = await loadMathJax();
  const text = source.trim();
  const frag = document.createDocumentFragment();
  if (!text) {
    target.replaceChildren();
    return;
  }
  const sole = isMixedContent(text) ? soleDisplayMath(text) : text;
  if (sole !== null) {
    frag.appendChild(await svgFor(mj, sole, true));
  } else {
    for (const para of text.split(/\n{2,}/)) {
      const p = document.createElement("p");
      for (const seg of splitMath(para)) {
        if (seg.type === "text") {
          p.insertAdjacentHTML("beforeend", escapeHtml(seg.value).replace(/\n/g, "<br>"));
        } else {
          p.appendChild(await svgFor(mj, seg.value, seg.display));
        }
      }
      frag.appendChild(p);
    }
  }
  target.replaceChildren(frag);
}

const FIT = { min: 16, max: 72, fillWidth: 0.9, fillHeight: 0.62 };

/**
 * Size a lone formula in `target` to fill its scrolling `area`: font-size drives the SVG's ex units,
 * so the formula grows and shrinks with the area. Mixed content keeps its text size.
 */
export function fitMath(target: HTMLElement, area: HTMLElement): void {
  const svg = target.querySelector<SVGSVGElement>(":scope > mjx-container > svg");
  target.classList.toggle("fit", !!svg);
  if (!svg) {
    target.style.removeProperty("font-size");
    return;
  }
  // Natural size in ex. Tagged equations are width="100%" with their real width as min-width.
  const ex = (s: string | null | undefined) => (s && s.trim().endsWith("ex") ? parseFloat(s) : NaN);
  const wEx = ex(svg.getAttribute("width")) || ex(svg.style.minWidth);
  const hEx = ex(svg.getAttribute("height"));
  const exPx = svg.getBoundingClientRect().height / hEx;
  if (!(wEx > 0 && hEx > 0 && exPx > 0)) return;
  const fontPx = parseFloat(getComputedStyle(target).fontSize);
  const pad = getComputedStyle(target);
  const availW = area.clientWidth - parseFloat(pad.paddingLeft) - parseFloat(pad.paddingRight);
  const availH = area.clientHeight - parseFloat(pad.paddingTop) - parseFloat(pad.paddingBottom);
  // ex scales with font-size, so the size that fits is a straight ratio.
  const k = Math.min((availW * FIT.fillWidth) / (wEx * exPx), (availH * FIT.fillHeight) / (hEx * exPx));
  const size = Math.max(FIT.min, Math.min(FIT.max, fontPx * k));
  target.style.fontSize = `${size.toFixed(1)}px`;
}

/** Keep `fitMath` applied while `area` resizes; call the returned function after each render. */
export function autoFit(target: HTMLElement, area: HTMLElement): () => void {
  const refit = () => fitMath(target, area);
  let last = "";
  new ResizeObserver(() => {
    const size = `${area.clientWidth}x${area.clientHeight}`;
    if (size !== last) {
      last = size;
      refit();
    }
  }).observe(area);
  return refit;
}

export async function toSvg(latex: string): Promise<string> {
  const mj = await loadMathJax();
  const node = await svgFor(mj, latex.trim(), true);
  const svg = node.querySelector("svg")!;
  svg.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  return svg.outerHTML;
}

export async function toMathML(latex: string): Promise<string> {
  const mj = await loadMathJax();
  mj.texReset();
  return mj.tex2mmlPromise(latex.trim(), { display: true });
}

/** Self-contained HTML: math is pre-rendered to inline SVG so it opens anywhere. */
export async function toHtmlDocument(content: string): Promise<string> {
  const holder = document.createElement("div");
  await renderInto(holder, content);
  return `<!doctype html>\n<html><head><meta charset="utf-8"><title>LaTeXSnipper export</title>
<style>body{font:16px/1.6 -apple-system,system-ui,sans-serif;max-width:46rem;margin:3rem auto;padding:0 1rem}mjx-container[display=true]{display:block;text-align:center;margin:1em 0}</style>
</head><body>${holder.innerHTML}</body></html>\n`;
}
