import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "../apps/web/acceptance",
  testMatch: "launcher-integrated.spec.ts",
  timeout: 150_000,
  expect: { timeout: 20_000 },
  workers: 1,
  retries: 0,
  outputDir: "../test-results/launcher-integrated",
  use: {
    baseURL: "https://moenarch.com/scrabble/",
    trace: "off",
    screenshot: "off",
  },
});
