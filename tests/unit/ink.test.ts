import { describe, expect, it } from "vitest";
import { InkModel } from "../../frontend/lib/ink";

describe("InkModel", () => {
  it("records strokes and drops jitter points", () => {
    const ink = new InkModel();
    ink.begin([0, 0]);
    ink.extend([0.2, 0.2]);
    ink.extend([5, 5]);
    expect(ink.strokes[0].points).toEqual([[0, 0], [5, 5]]);
    expect(ink.isEmpty()).toBe(false);
  });

  it("computes padded bounds", () => {
    const ink = new InkModel();
    expect(ink.bounds()).toBeNull();
    ink.begin([10, 20]);
    ink.extend([30, 60]);
    expect(ink.bounds(5)).toEqual({ x: 5, y: 15, w: 30, h: 50 });
  });

  it("erases touched strokes and undoes", () => {
    const ink = new InkModel();
    ink.begin([0, 0]);
    ink.extend([10, 0]);
    ink.begin([100, 100]);
    expect(ink.erase([50, 50])).toBe(false);
    expect(ink.erase([9, 1])).toBe(true);
    expect(ink.strokes).toHaveLength(1);
    expect(ink.undo()).toBe(true);
    expect(ink.strokes).toHaveLength(2);
    ink.clear();
    expect(ink.isEmpty()).toBe(true);
    ink.undo();
    expect(ink.strokes).toHaveLength(2);
    while (ink.undo());
    expect(ink.isEmpty()).toBe(true);
  });

  it("ignores extend without an active stroke", () => {
    const ink = new InkModel();
    ink.extend([1, 1]);
    expect(ink.isEmpty()).toBe(true);
  });
});
