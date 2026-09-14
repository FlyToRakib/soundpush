// End-to-end UI tests (plan §29.1 "Desktop UI"): the built UI in Chromium against the in-browser
// mock engine (src/lib/engine/mock.ts), in light and dark themes, with axe accessibility checks.
// Run with `npm run test:e2e` (first time: `npx playwright install chromium`).
import { defineConfig, devices } from "@playwright/test";

const PORT = 4173;

export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    // The window's default size (src-tauri main.rs).
    viewport: { width: 1000, height: 700 },
    trace: "retain-on-failure",
  },
  projects: [
    { name: "light", use: { ...devices["Desktop Chrome"], viewport: { width: 1000, height: 700 }, colorScheme: "light" } },
    { name: "dark", use: { ...devices["Desktop Chrome"], viewport: { width: 1000, height: 700 }, colorScheme: "dark" } },
  ],
  webServer: {
    command: `npm run build && npx vite preview --host 127.0.0.1 --port ${PORT} --strictPort`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
