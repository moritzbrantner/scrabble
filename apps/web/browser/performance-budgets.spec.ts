import { expect, test } from "@playwright/test";

// Browser-local responsive interaction budgets. These are not Playwright's test timeout.
const INTERACTION_BUDGET_MS = 50;
// Shared CI is not a stable reference device. Explicitly enable the hard gate
// only on an owned/controlled runner; always check semantic correctness.
const enforceTiming = process.env.SCRABBLE_PERF_ENFORCE === "1";

function enforceBudget(observed: number, label: string) {
  if (enforceTiming) {
    expect(observed, label).toBeLessThanOrEqual(INTERACTION_BUDGET_MS);
  }
}

function p95(samples: number[]): number {
  expect(samples.length).toBeGreaterThanOrEqual(12);
  const sorted = [...samples].sort((left, right) => left - right);
  return sorted[Math.ceil(sorted.length * 0.95) - 1]!;
}

test("50 ms p95: rendering changed Scrabble board snapshots", async ({ page }) => {
  await page.goto("browser/performance.html");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();

  const samples = await page.evaluate(() => {
    const fixture = window.scrabblePerformance;
    if (fixture === undefined) throw new Error("Optimized board performance fixture is unavailable");
    return fixture.measureBoardRefreshes();
  });
  const observed = p95(samples);
  console.log(`Scrabble board React refresh: p95=${observed.toFixed(2)}ms, budget=50ms`);
  enforceBudget(observed, "React state update, reconcile and layout of the 225-square public board");
});

test("50 ms p95: placing word tiles and clearing a move draft", async ({ page }) => {
  await page.goto("browser/performance.html");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();

  const { placeMs, resetMs } = await page.evaluate(() => {
    const fixture = window.scrabblePerformance;
    if (fixture === undefined) {
      throw new Error("Optimized move-editor performance fixture is unavailable");
    }
    return fixture.measureWordDraftInteractions();
  });
  const placement = p95(placeMs);
  const reset = p95(resetMs);
  console.log(
    `Scrabble word-draft placement: p95=${placement.toFixed(2)}ms; reset: p95=${reset.toFixed(2)}ms; budget=50ms`,
  );
  enforceBudget(placement, "Select rack tile → place square → updated 225-square editor");
  enforceBudget(reset, "Cancel draft → board restored");
});
