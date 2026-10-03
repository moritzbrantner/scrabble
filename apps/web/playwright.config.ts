import { defineConfig } from "@playwright/test";
const port = process.env.SCRABBLE_BROWSER_PORT;
if (port === undefined || !/^[0-9]+$/.test(port)) {
  throw new Error("Use bun run test:browser to provision the shared test port");
}
export default defineConfig({
  testDir: "./browser",
  timeout: 30_000,
  retries: 0,
  workers: 1,
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
