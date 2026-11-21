import { defineConfig, devices } from "@playwright/test";

// The original add-in pages against this repository's host, in headless
// WebKit (the engine of Office for Mac). The dev server uses the real
// services with a throwaway certificate; nothing is installed.
const PORT = 28799;

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  reporter: [["list"]],
  use: { baseURL: `https://localhost:${PORT}`, ignoreHTTPSErrors: true, headless: true, locale: "zh-CN" },
  projects: [{ name: "webkit", use: { ...devices["Desktop Safari"], ignoreHTTPSErrors: true } }],
  webServer: {
    // Assemble and build first, so the pages tested are the ones in the source tree.
    command: `node tools/assemble.mjs && npm run ui && cargo run --quiet --manifest-path host/Cargo.toml --example dev_server ${PORT}`,
    url: `https://localhost:${PORT}/addin/help.html`,
    ignoreHTTPSErrors: true,
    reuseExistingServer: false,
    timeout: 600_000,
  },
});
