import { defineConfig, devices } from "@playwright/test";

// The UI runs against e2e/tauri-mock.js, an in-memory stand-in for the Rust commands.
// Always headless: no browser window, focus change or input on the desktop.
export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  fullyParallel: true,
  reporter: [["list"]],
  use: { baseURL: "http://localhost:1420", headless: true, viewport: { width: 1160, height: 720 }, trace: "retain-on-failure" },
  // WebKit is the engine of macOS (WKWebView) and Linux (WebKitGTK) builds;
  // Chromium is the engine of the Windows build (WebView2).
  projects: [
    { name: "webkit", use: { ...devices["Desktop Safari"], headless: true, viewport: { width: 1160, height: 720 } } },
    { name: "chromium", use: { ...devices["Desktop Chrome"], headless: true, viewport: { width: 1160, height: 720 } } },
  ],
  webServer: { command: "npx vite --port 1420 --strictPort", url: "http://localhost:1420", reuseExistingServer: true },
});
