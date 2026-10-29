import { defineConfig } from "@playwright/test";
const port = process.env.SCRABBLE_BROWSER_PORT;
if (port === undefined || !/^[0-9]+$/.test(port)) {
  throw new Error("Use bun run test:browser to provision the shared test port");
}
export default defineConfig({
  testDir: "./browser",
  outputDir: process.env.SCRABBLE_PRESET_ACCEPTANCE_ROOT
    ? "../../test-results/prepared-presets"
    : "../../test-results/browser",
  // Pages-only specs run against built bundles in playwright.pages.config.ts.
  testIgnore: [
    "**/playable-loop.spec.ts",
    "**/production-configuration.spec.ts",
    "**/performance-budgets.spec.ts",
    "**/rematch.spec.ts",
  ],
  timeout: 30_000,
  retries: 0,
  // Each worker owns a private native fixture (browser/fixtures.ts).
  workers: process.env.CI ? 4 : "50%",
  use: {
    baseURL: `http://127.0.0.1:${port}/scrabble/`,
    viewport: { width: 1280, height: 900 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: `bun run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}/scrabble/`,
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
