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
    "**/hosted-account-cookie.spec.ts",
  ],
  timeout: 30_000,
  retries: 0,
  fullyParallel: true,
  // Each worker owns a private native fixture (browser/fixtures.ts). CI stays serial per runner,
  // because timing-sensitive specs flake when two workers share its 4 vCPUs. It shards the
  // suite across runners instead (validate.yml, #222).
  workers: process.env.CI ? 1 : "50%",
  use: {
    baseURL: `http://127.0.0.1:${port}/scrabble/`,
    viewport: { width: 1280, height: 900 },
    // Recording a trace for every test roughly doubled the suite's Chromium CPU (#222). CI keeps
    // failure screenshots; rerun a failing spec locally (or with --trace on) for its trace.
    trace: process.env.CI ? "off" : "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: `bun run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}/scrabble/`,
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
