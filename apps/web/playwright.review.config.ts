import { defineConfig } from "@playwright/test";

import development from "./playwright.config";

// Opt-in owner review scenarios (#185). Never part of `bun run check` or CI.
export default defineConfig({
  ...development,
  testDir: "./review",
  testMatch: "**/*.review.ts",
  testIgnore: [],
  outputDir: "../../test-results/review",
  timeout: 60_000,
});
