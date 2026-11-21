import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";

// The Setup window's page; Tauri bundles build/ui.
export default defineConfig({
  root: fileURLToPath(new URL("./ui", import.meta.url)),
  base: "./",
  clearScreen: false,
  build: {
    target: "safari16",
    outDir: fileURLToPath(new URL("../build/ui", import.meta.url)),
    emptyOutDir: true,
  },
});
