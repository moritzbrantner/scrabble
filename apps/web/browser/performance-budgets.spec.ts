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

test("50 ms p95: reading the rendered public board", async ({ page }) => {
  await page.goto("?fixture=playing");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();

  const samples = await page.evaluate(async () => {
    const path = "/scrabble/browser/performance-fixture.tsx";
    const { measureBoardReads } = (await import(path)) as typeof import("./performance-fixture");
    return measureBoardReads();
  });
  const observed = p95(samples);
  console.log(`Scrabble public board read: p95=${observed.toFixed(2)}ms, budget=50ms`);
  enforceBudget(observed, "Read the 225-square public board DOM and layout");
});

test("50 ms p95: placing word tiles and clearing a move draft", async ({ page }) => {
  await page.goto("?fixture=playing");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();

  const { placeMs, resetMs } = await page.evaluate(async () => {
    const path = "/scrabble/browser/performance-fixture.tsx";
    const { measureWordDraftInteractions } = (await import(
      path
    )) as typeof import("./performance-fixture");
    return measureWordDraftInteractions();
  });
  const placement = p95(placeMs);
  const reset = p95(resetMs);
  console.log(
    `Scrabble word-draft placement: p95=${placement.toFixed(2)}ms; reset: p95=${reset.toFixed(2)}ms; budget=50ms`,
  );
  enforceBudget(placement, "Select rack tile → place square → updated 225-square editor");
  enforceBudget(reset, "Cancel draft → board restored");
});
