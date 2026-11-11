// Spotlight tour: dims the window, cuts a hole around the real control a step
// talks about, and puts the explanation in a card beside it.

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export type Side = "bottom" | "top" | "right" | "left" | "inside" | "center";

/** Smallest box containing all boxes; null when there are none. */
export function unionBox(boxes: Box[]): Box | null {
  if (!boxes.length) return null;
  const left = Math.min(...boxes.map((b) => b.left));
  const top = Math.min(...boxes.map((b) => b.top));
  const right = Math.max(...boxes.map((b) => b.left + b.width));
  const bottom = Math.max(...boxes.map((b) => b.top + b.height));
  return { left, top, width: right - left, height: bottom - top };
}

/** Grow a box by `pad`, then keep it `inset` px inside the viewport so its ring stays visible. */
export function holeFor(target: Box, view: { width: number; height: number }, pad = 6, inset = 4): Box {
  const left = Math.max(inset, target.left - pad);
  const top = Math.max(inset, target.top - pad);
  const right = Math.min(view.width - inset, target.left + target.width + pad);
  const bottom = Math.min(view.height - inset, target.top + target.height + pad);
  return { left, top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) };
}

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));

/**
 * Where the card goes: the first side of the target with room for it
 * (below, above, right, left), else inside the target near the bottom.
 * Without a target the card is centered.
 */
export function placeCard(
  target: Box | null,
  card: { width: number; height: number },
  view: { width: number; height: number },
  gap = 14,
  margin = 12,
): { left: number; top: number; side: Side } {
  const centeredLeft = clamp((view.width - card.width) / 2, margin, view.width - card.width - margin);
  if (!target) {
    return { left: centeredLeft, top: clamp((view.height - card.height) / 2, margin, view.height - card.height - margin), side: "center" };
  }
  const right = target.left + target.width;
  const bottom = target.top + target.height;
  const alongX = clamp(target.left + target.width / 2 - card.width / 2, margin, view.width - card.width - margin);
  const alongY = clamp(target.top + target.height / 2 - card.height / 2, margin, view.height - card.height - margin);
  if (view.height - bottom - gap - margin >= card.height) return { left: alongX, top: bottom + gap, side: "bottom" };
  if (target.top - gap - margin >= card.height) return { left: alongX, top: target.top - gap - card.height, side: "top" };
  if (view.width - right - gap - margin >= card.width) return { left: right + gap, top: alongY, side: "right" };
  if (target.left - gap - margin >= card.width) return { left: target.left - gap - card.width, top: alongY, side: "left" };
  const inside = clamp(bottom - card.height - margin, margin, view.height - card.height - margin);
  return { left: alongX, top: inside, side: "inside" };
}

export interface TourStep {
  /** CSS selectors of the controls to highlight together; none centers the card. */
  targets: string[];
  title: string;
  body: string;
}

export interface TourText {
  title(step: TourStep): string;
  body(step: TourStep): string;
  skip: string;
  back: string;
  next: string;
  done: string;
}

export class SpotlightTour {
  private index = 0;
  private root: HTMLElement | null = null;
  private returnFocus: Element | null = null;
  private onResize = () => this.layout();

  constructor(
    private readonly steps: TourStep[],
    private readonly text: () => TourText,
    /** `completed` is true when the last step was confirmed, false when skipped. */
    private readonly onEnd: (completed: boolean) => void,
  ) {}

  get isOpen(): boolean {
    return this.root !== null;
  }

  get step(): number {
    return this.index;
  }

  start(): void {
    if (this.root) return this.go(0);
    this.returnFocus = document.activeElement;
    this.root = this.build();
    document.body.appendChild(this.root);
    window.addEventListener("resize", this.onResize);
    this.go(0, true);
  }

  end(completed: boolean): void {
    if (!this.root) return;
    window.removeEventListener("resize", this.onResize);
    this.root.remove();
    this.root = null;
    if (this.returnFocus instanceof HTMLElement && this.returnFocus.isConnected) this.returnFocus.focus();
    this.onEnd(completed);
  }

  go(index: number, instant = false): void {
    if (!this.root) return;
    this.index = Math.min(Math.max(index, 0), this.steps.length - 1);
    const step = this.steps[this.index];
    const text = this.text();
    const q = <T extends HTMLElement>(sel: string) => this.root!.querySelector<T>(sel)!;
    q(".tour-count").textContent = `${this.index + 1} / ${this.steps.length}`;
    q(".tour-title").textContent = text.title(step);
    q(".tour-body").textContent = text.body(step);
    q(".tour-skip").textContent = text.skip;
    const back = q<HTMLButtonElement>(".tour-back");
    back.textContent = text.back;
    back.disabled = this.index === 0;
    const last = this.index === this.steps.length - 1;
    const next = q<HTMLButtonElement>(".tour-next");
    next.textContent = last ? text.done : text.next;
    // Jumps (first show, resize) skip the glide between controls.
    this.root.classList.toggle("instant", instant);
    this.layout();
    if (instant) requestAnimationFrame(() => this.root?.classList.remove("instant"));
    next.focus();
  }

  /** Recompute the hole and card for the current step from live element positions. */
  layout(): void {
    if (!this.root) return;
    const step = this.steps[this.index];
    const view = { width: window.innerWidth, height: window.innerHeight };
    const boxes = step.targets
      .map((sel) => document.querySelector(sel)?.getBoundingClientRect())
      .filter((r): r is DOMRect => !!r && r.width > 0 && r.height > 0)
      .map((r) => ({ left: r.left, top: r.top, width: r.width, height: r.height }));
    const target = unionBox(boxes);
    const hole = this.root.querySelector<HTMLElement>(".tour-hole")!;
    const card = this.root.querySelector<HTMLElement>(".tour-card")!;
    this.root.classList.toggle("centered", !target);
    let holeBox: Box | null = null;
    if (target) {
      holeBox = holeFor(target, view);
      Object.assign(hole.style, {
        left: `${holeBox.left}px`,
        top: `${holeBox.top}px`,
        width: `${holeBox.width}px`,
        height: `${holeBox.height}px`,
      });
    }
    const pos = placeCard(holeBox, { width: card.offsetWidth, height: card.offsetHeight }, view);
    card.style.left = `${pos.left}px`;
    card.style.top = `${pos.top}px`;
    card.dataset.side = pos.side;
  }

  private build(): HTMLElement {
    const root = document.createElement("div");
    root.className = "tour";
    root.setAttribute("role", "dialog");
    root.setAttribute("aria-modal", "true");
    root.setAttribute("aria-labelledby", "tour-title");
    root.setAttribute("aria-describedby", "tour-body");
    root.innerHTML = `
      <div class="tour-drag" data-tauri-drag-region></div>
      <div class="tour-hole" aria-hidden="true"></div>
      <section class="tour-card">
        <div aria-live="polite">
          <p class="tour-count"></p>
          <h3 class="tour-title" id="tour-title"></h3>
          <p class="tour-body" id="tour-body"></p>
        </div>
        <footer>
          <button type="button" class="btn quiet small tour-skip"></button>
          <button type="button" class="btn quiet tour-back"></button>
          <button type="button" class="btn primary tour-next"></button>
        </footer>
      </section>`;
    root.querySelector(".tour-skip")!.addEventListener("click", () => this.end(false));
    root.querySelector(".tour-back")!.addEventListener("click", () => this.go(this.index - 1));
    root.querySelector(".tour-next")!.addEventListener("click", () => {
      if (this.index === this.steps.length - 1) this.end(true);
      else this.go(this.index + 1);
    });
    // The highlighted controls are for looking at: clicks never reach the app underneath.
    root.addEventListener("pointerdown", (e) => {
      if (!(e.target as HTMLElement).closest(".tour-card, .tour-drag")) e.preventDefault();
    });
    root.addEventListener("keydown", (e) => this.onKey(e));
    return root;
  }

  private onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      this.end(false);
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      if (this.index < this.steps.length - 1) this.go(this.index + 1);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      if (this.index > 0) this.go(this.index - 1);
    } else if (e.key === "Tab") {
      // Keep focus on the card's buttons.
      const buttons = [...this.root!.querySelectorAll<HTMLButtonElement>(".tour-card button:not(:disabled)")];
      const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
      e.preventDefault();
      buttons[(at + (e.shiftKey ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    }
  }
}
