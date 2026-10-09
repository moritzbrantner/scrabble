import { expect, test } from "@playwright/test";

// Browser-local responsive interaction budgets. These are not Playwright's test timeout.
const INTERACTION_BUDGET_MS = 50;

function p95(samples: number[]): number {
  expect(samples.length).toBeGreaterThanOrEqual(12);
  const sorted = [...samples].sort((left, right) => left - right);
  return sorted[Math.ceil(sorted.length * 0.95) - 1]!;
}

test("50 ms p95: reading a refreshed public board", async ({ page }) => {
  await page.goto("?fixture=playing");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();

  const samples = await page.evaluate(async () => {
    const path = "/scrabble/browser/performance-fixture.tsx";
    const { measureBoardRefreshes } = (await import(path)) as typeof import("./performance-fixture");
    return measureBoardRefreshes();
  });
  const observed = p95(samples);
  console.log(`Scrabble board refresh: p95=${observed.toFixed(2)}ms, budget=50ms`);
  expect(observed, "Warm 225-square React update and layout").toBeLessThanOrEqual(
    INTERACTION_BUDGET_MS,
  );
});

test("50 ms p95: placing word tiles and clearing a move draft", async ({ page }) => {
  await page.goto("?fixture=playing");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();

  const { placeMs, resetMs } = await page.evaluate(async () => {
    const path = "/scrabble/browser/performance-fixture.tsx";
    const { measureWordDraftInteractions } = (await import(path)) as typeof import("./performance-fixture");
    return measureWordDraftInteractions();
  });
  const placement = p95(placeMs);
  const reset = p95(resetMs);
  console.log(
    `Scrabble word-draft placement: p95=${placement.toFixed(2)}ms; reset: p95=${reset.toFixed(2)}ms; budget=50ms`,
  );
  expect(placement, "Select rack tile → place square → updated 225-square editor").toBeLessThanOrEqual(
    INTERACTION_BUDGET_MS,
  );
  expect(reset, "Cancel draft → board restored").toBeLessThanOrEqual(INTERACTION_BUDGET_MS);
});
