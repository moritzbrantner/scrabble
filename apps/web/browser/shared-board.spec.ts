import { test, expect } from "@playwright/test";

test("every phase renders with stable board geometry and read-only semantics", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let first: { x: number; y: number; width: number; height: number } | null = null;
  for (const fixture of ["lobby", "playing", "finished"]) {
    await page.goto(`?fixture=${fixture}`);
    const board = page.getByRole("table", { name: "Scrabble board" });
    await expect(board.getByRole("cell")).toHaveCount(225);
    await expect(board.getByRole("button")).toHaveCount(0);
    const corner = page.getByRole("cell", { name: "Row 1, column 1: Triple word", exact: true });
    const bounds = await corner.boundingBox();
    expect(bounds).not.toBeNull();
    if (bounds) {
      expect(Math.abs(bounds.width - bounds.height)).toBeLessThan(1);
      if (first) {
        expect(bounds.x).toBeCloseTo(first.x, 1);
        expect(bounds.y).toBeCloseTo(first.y, 1);
        expect(bounds.width).toBeCloseTo(first.width, 1);
      }
      first = bounds;
    }
    if (fixture === "playing") {
      await expect(page.getByRole("status")).toHaveText("Ada's turn");
      await expect(
        page.getByRole("cell", { name: /Row 8, column 8: A, 0 points, blank/ }),
      ).toBeVisible();
    }
    const colors = new Set<string>();
    for (const name of [
      "Row 1, column 1: Triple word",
      "Row 1, column 2: Normal square",
      "Row 1, column 4: Double letter",
      "Row 2, column 2: Double word",
      "Row 2, column 6: Triple letter",
    ]) {
      colors.add(
        await page
          .getByRole("cell", { name, exact: true })
          .evaluate(
            (element) => getComputedStyle(element.firstElementChild ?? element).backgroundColor,
          ),
      );
    }
    expect(colors.size).toBe(5);
    await page.screenshot({ path: `test-results/shared-board-${fixture}.png`, fullPage: true });
    if (fixture === "finished") {
      await expect(page.getByRole("status")).toHaveText("Winner: Ada");
    }
  }
  expect(errors).toEqual([]);
});
test("board stays within a narrow viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("?fixture=playing");
  const board = page.getByRole("table", { name: "Scrabble board" });
  await expect(board).toBeVisible();
  const bounds = await board.boundingBox();
  expect(bounds).not.toBeNull();
  if (bounds) {
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(390);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
});
