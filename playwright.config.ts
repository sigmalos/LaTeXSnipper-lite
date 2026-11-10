import { defineConfig, devices } from "@playwright/test";

// The UI runs in WebKit — the same engine as Tauri's WKWebView on macOS —
// against e2e/tauri-mock.js, an in-memory stand-in for the Rust commands.
export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  fullyParallel: true,
  reporter: [["list"]],
  use: { baseURL: "http://localhost:1420", viewport: { width: 1160, height: 720 }, trace: "retain-on-failure" },
  projects: [{ name: "webkit", use: { ...devices["Desktop Safari"], viewport: { width: 1160, height: 720 } } }],
  webServer: { command: "npx vite --port 1420 --strictPort", url: "http://localhost:1420", reuseExistingServer: true },
});
