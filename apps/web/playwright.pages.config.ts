import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

import development from "./playwright.config";
const port = process.env.SCRABBLE_BROWSER_PORT;
const suite = process.env.SCRABBLE_BROWSER_SUITE;
export default defineConfig({
  ...development,
  testIgnore:
    suite === "other"
      ? ["**/hexagonal-board.spec.ts", "**/custom-board-layout-replay.spec.ts"]
      : [],
  testMatch:
    suite === "custom-layout-replay"
      ? ["**/custom-board-layout-replay.spec.ts"]
      : suite === "hex"
        ? ["**/hexagonal-board.spec.ts"]
        : [
            "**/playable-loop.spec.ts",
            "**/production-configuration.spec.ts",
            "**/performance-budgets.spec.ts",
            "**/rematch.spec.ts",
            "**/profile-creation.spec.ts",
            "**/custom-board-layout-responsive.spec.ts",
            "**/custom-board-layout-replay.spec.ts",
            "**/multi-letter-tiles.spec.ts",
            "**/hexagonal-board.spec.ts",
            "**/match-clock.spec.ts",
          ],
  outputDir: suite
    ? `../../test-results/pages-browser-${suite}`
    : "../../test-results/pages-browser",
  webServer: {
    cwd: fileURLToPath(new URL("../..", import.meta.url)),
    command: `bunx vite preview --config apps/web/vite.config.ts --outDir ../../test-results/pages-build --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}/scrabble/`,
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
