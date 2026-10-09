import { defineConfig } from "@playwright/test";
import { fileURLToPath } from "node:url";
import development from "./playwright.config";
const port = process.env.SCRABBLE_BROWSER_PORT;
export default defineConfig({
  ...development,
  testIgnore: [],
  testMatch: [
    "**/playable-loop.spec.ts",
    "**/production-configuration.spec.ts",
    "**/performance-budgets.spec.ts",
  ],
  outputDir: "../../test-results/pages-browser",
  webServer: {
    cwd: fileURLToPath(new URL("../..", import.meta.url)),
    command: `bunx vite preview --config apps/web/vite.config.ts --outDir ../../test-results/pages-build --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}/scrabble/`,
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
