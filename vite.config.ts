/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const page = (name: string) => fileURLToPath(new URL(name, import.meta.url));

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "safari16",
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 2600,
    // The result window (result.html) is its own page next to the main one.
    rollupOptions: { input: { main: page("index.html"), result: page("result.html") } },
  },
  test: {
    environment: "jsdom",
    include: ["tests/unit/**/*.test.ts"],
  },
});
