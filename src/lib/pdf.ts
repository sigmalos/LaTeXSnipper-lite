// PDF pages → PNG data URLs via pdf.js (loaded only when a PDF is opened).
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

type PdfDoc = { numPages: number; getPage(n: number): Promise<PdfPage>; destroy(): Promise<void> };
type PdfPage = {
  getViewport(o: { scale: number }): { width: number; height: number };
  render(o: { canvasContext: CanvasRenderingContext2D; viewport: unknown }): { promise: Promise<void> };
};

export async function openPdf(bytes: ArrayBuffer): Promise<PdfDoc> {
  const pdfjs = await import("pdfjs-dist");
  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
  return (await pdfjs.getDocument({ data: new Uint8Array(bytes) }).promise) as unknown as PdfDoc;
}

export async function renderPage(doc: PdfDoc, page: number, dpi: number): Promise<string> {
  const p = await doc.getPage(page);
  const viewport = p.getViewport({ scale: dpi / 72 });
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(viewport.width);
  canvas.height = Math.ceil(viewport.height);
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  await p.render({ canvasContext: ctx, viewport }).promise;
  return canvas.toDataURL("image/png");
}
