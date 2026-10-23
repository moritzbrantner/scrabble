import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

import development from "./playwright.config";
const port = process.env.SCRABBLE_BROWSER_PORT;
const suite = process.env.SCRABBLE_BROWSER_SUITE;
function selectedTests() {
  if (suite === "custom-layout-replay") {
    return ["**/custom-board-layout-replay.spec.ts"];
  }
  if (suite === "hex") {
    return ["**/hexagonal-board.spec.ts"];
  }
  return [
    "**/playable-loop.spec.ts",
    "**/production-configuration.spec.ts",
    "**/performance-budgets.spec.ts",
    "**/rematch.spec.ts",
    "**/profile-creation.spec.ts",
    "**/word-list-presets.spec.ts",
    "**/custom-board-layout-responsive.spec.ts",
    "**/custom-word-lists.spec.ts",
    "**/custom-tile-profiles.spec.ts",
    "**/custom-board-layout-replay.spec.ts",
    "**/multi-letter-tiles.spec.ts",
    "**/hexagonal-board.spec.ts",
    "**/match-clock.spec.ts",
  ];
}
export default defineConfig({
  ...development,
  testIgnore:
    suite === "other"
      ? ["**/hexagonal-board.spec.ts", "**/custom-board-layout-replay.spec.ts"]
      : [],
  testMatch: selectedTests(),
  outputDir: suite
    ? `../../test-results/pages-browser-${suite}`
    : "../../test-results/pages-browser",
  ...(process.env.SCRABBLE_PRESET_ACCEPTANCE_ROOT
    ? { outputDir: "../../test-results/pages-prepared-presets" }
    : {}),
  webServer: {
    cwd: fileURLToPath(new URL("../..", import.meta.url)),
    command: `bunx vite preview --config apps/web/vite.config.ts --outDir ../../test-results/pages-build --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}/scrabble/`,
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
