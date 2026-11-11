import { afterEach, describe, expect, it } from "vitest";
import { holeFor, placeCard, SpotlightTour, unionBox, type TourStep } from "../../frontend/lib/tour";

const view = { width: 1160, height: 720 };
const card = { width: 300, height: 150 };

describe("unionBox", () => {
  it("spans every box", () => {
    expect(unionBox([{ left: 10, top: 20, width: 30, height: 10 }, { left: 100, top: 5, width: 10, height: 50 }])).toEqual({
      left: 10,
      top: 5,
      width: 100,
      height: 50,
    });
    expect(unionBox([])).toBeNull();
  });
});

describe("holeFor", () => {
  it("pads the target", () => {
    expect(holeFor({ left: 100, top: 100, width: 50, height: 20 }, view)).toEqual({ left: 94, top: 94, width: 62, height: 32 });
  });
  it("stays inside the window so the ring is visible", () => {
    const h = holeFor({ left: 0, top: 0, width: 1160, height: 720 }, view);
    expect(h).toEqual({ left: 4, top: 4, width: 1152, height: 712 });
  });
});

describe("placeCard", () => {
  it("prefers below, aligned under the target and clamped to the window", () => {
    expect(placeCard({ left: 12, top: 38, width: 380, height: 40 }, card, view)).toEqual({ left: 52, top: 92, side: "bottom" });
    const right = placeCard({ left: 1100, top: 10, width: 50, height: 30 }, card, view);
    expect(right.left).toBe(1160 - 300 - 12);
  });
  it("goes above when there is no room below", () => {
    const p = placeCard({ left: 12, top: 640, width: 300, height: 60 }, card, view);
    expect(p).toEqual({ left: 12, top: 640 - 14 - 150, side: "top" });
  });
  it("goes beside tall targets", () => {
    expect(placeCard({ left: 8, top: 100, width: 330, height: 560 }, card, view).side).toBe("right");
    const left = placeCard({ left: 400, top: 40, width: 740, height: 660 }, card, view);
    expect(left.side).toBe("left");
    expect(left.left).toBe(400 - 14 - 300);
  });
  it("falls back inside a target that leaves no room anywhere", () => {
    const p = placeCard({ left: 4, top: 4, width: 1152, height: 712 }, card, view);
    expect(p.side).toBe("inside");
    expect(p.top + card.height).toBeLessThanOrEqual(720 - 12);
  });
  it("centers without a target", () => {
    expect(placeCard(null, card, view)).toEqual({ left: 430, top: 285, side: "center" });
  });
  it("never leaves the window, even when the window is smaller than the card", () => {
    const p = placeCard(null, card, { width: 200, height: 100 });
    expect(p.left).toBe(12);
    expect(p.top).toBe(12);
  });
});

describe("SpotlightTour", () => {
  const steps: TourStep[] = [
    { targets: ["#a"], title: "A", body: "first" },
    { targets: ["#missing"], title: "B", body: "second" },
  ];
  const text = () => ({ title: (s: TourStep) => s.title, body: (s: TourStep) => s.body, skip: "Skip", back: "Back", next: "Next", done: "Done" });
  afterEach(() => document.body.replaceChildren());

  it("walks the steps and reports completion", () => {
    document.body.innerHTML = `<button id="a">a</button>`;
    const ends: boolean[] = [];
    const tour = new SpotlightTour(steps, text, (done) => ends.push(done));
    tour.start();
    expect(tour.isOpen).toBe(true);
    expect(document.querySelector(".tour-title")!.textContent).toBe("A");
    (document.querySelector(".tour-next") as HTMLButtonElement).click();
    expect(document.querySelector(".tour-next")!.textContent).toBe("Done");
    // A missing target centers the card instead of failing.
    expect(document.querySelector(".tour")!.classList.contains("centered")).toBe(true);
    (document.querySelector(".tour-next") as HTMLButtonElement).click();
    expect(tour.isOpen).toBe(false);
    expect(document.querySelector(".tour")).toBeNull();
    expect(ends).toEqual([true]);
  });

  it("Escape skips and restores focus", () => {
    document.body.innerHTML = `<button id="a">a</button>`;
    const a = document.getElementById("a")!;
    a.focus();
    const ends: boolean[] = [];
    const tour = new SpotlightTour(steps, text, (done) => ends.push(done));
    tour.start();
    expect(document.activeElement?.className).toContain("tour-next");
    document.querySelector(".tour")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(ends).toEqual([false]);
    expect(document.activeElement).toBe(a);
  });

  it("starting again while open restarts instead of stacking overlays", () => {
    document.body.innerHTML = `<button id="a">a</button>`;
    const tour = new SpotlightTour(steps, text, () => undefined);
    tour.start();
    tour.go(1);
    tour.start();
    expect(document.querySelectorAll(".tour")).toHaveLength(1);
    expect(tour.step).toBe(0);
  });
});
