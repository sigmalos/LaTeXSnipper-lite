// Handwriting stroke model, kept separate from the canvas for testing.
export type Point = [number, number];
export interface Stroke {
  points: Point[];
  width: number;
}

export class InkModel {
  strokes: Stroke[] = [];
  private undoStack: Stroke[][] = [];

  begin(p: Point, width = 3.2): void {
    this.snapshot();
    this.strokes.push({ points: [p], width });
  }

  extend(p: Point): void {
    const s = this.strokes[this.strokes.length - 1];
    if (!s) return;
    const last = s.points[s.points.length - 1];
    if (Math.hypot(p[0] - last[0], p[1] - last[1]) >= 0.8) s.points.push(p);
  }

  /** Remove every stroke passing within `radius` of `p`. Returns whether anything changed. */
  erase(p: Point, radius = 10, record = true): boolean {
    const keep = this.strokes.filter((s) => !s.points.some((q) => Math.hypot(q[0] - p[0], q[1] - p[1]) <= radius));
    if (keep.length === this.strokes.length) return false;
    if (record) this.snapshot();
    this.strokes = keep;
    return true;
  }

  undo(): boolean {
    const prev = this.undoStack.pop();
    if (!prev) return false;
    this.strokes = prev;
    return true;
  }

  clear(): void {
    if (this.strokes.length) this.snapshot();
    this.strokes = [];
  }

  isEmpty(): boolean {
    return this.strokes.length === 0;
  }

  /** Bounding box of all ink, padded; null when empty. */
  bounds(pad = 16): { x: number; y: number; w: number; h: number } | null {
    const pts = this.strokes.flatMap((s) => s.points);
    if (!pts.length) return null;
    const xs = pts.map((p) => p[0]);
    const ys = pts.map((p) => p[1]);
    const x = Math.min(...xs) - pad;
    const y = Math.min(...ys) - pad;
    return { x, y, w: Math.max(...xs) + pad - x, h: Math.max(...ys) + pad - y };
  }

  private snapshot(): void {
    this.undoStack.push(this.strokes.map((s) => ({ ...s, points: [...s.points] })));
    if (this.undoStack.length > 100) this.undoStack.shift();
  }
}

export function drawStrokes(ctx: CanvasRenderingContext2D, strokes: Stroke[], color: string, dx = 0, dy = 0): void {
  ctx.strokeStyle = color;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const s of strokes) {
    ctx.lineWidth = s.width;
    ctx.beginPath();
    s.points.forEach(([x, y], i) => (i ? ctx.lineTo(x - dx, y - dy) : ctx.moveTo(x - dx, y - dy)));
    if (s.points.length === 1) ctx.lineTo(s.points[0][0] - dx + 0.1, s.points[0][1] - dy);
    ctx.stroke();
  }
}

/** Black-on-white PNG of the cropped ink, what the formula model expects. */
export function inkToPng(model: InkModel): string | null {
  const b = model.bounds();
  if (!b) return null;
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(b.w);
  canvas.height = Math.ceil(b.h);
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  drawStrokes(ctx, model.strokes, "#000", b.x, b.y);
  return canvas.toDataURL("image/png");
}
