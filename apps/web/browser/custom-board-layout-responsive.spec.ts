// Independent acceptance for #137, authored on 196d8cd before the layout fix.
// Real browser + native fixture; only presentation/navigation is exercised here.
import { expect, test, type Locator, type Page } from "./fixtures";

import { fixtureQuery } from "./game-fixture";

const editor = (page: Page) =>
  page.getByRole("region", { name: "Board layout editor", exact: true });
const grid = (page: Page) => editor(page).getByRole("grid", { name: "Layout cells", exact: true });
const cells = (page: Page) => grid(page).getByRole("button");
const cell = (page: Page, row: number, column: number) =>
  grid(page).getByRole("button", { name: new RegExp(`^Row ${row}, column ${column}:`) });

async function tabTo(page: Page, target: Locator) {
  for (let count = 0; count < 100; count += 1) {
    if (await target.evaluate((element) => element === document.activeElement)) {
      return;
    }
    await page.keyboard.press("Tab");
  }
  throw new Error("Editor control is not keyboard reachable");
}

async function fitsAndRemainsReachable(page: Page, width: number, count: number) {
  await expect(cells(page)).toHaveCount(count);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
    width,
  );
  // Check every actual cell's hit target after bringing it into view. This permits
  // a scrollable board, but fails hidden/clipped cells and overlapping controls.
  const inaccessible = await cells(page).evaluateAll((buttons) =>
    buttons.flatMap((button) => {
      button.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
      const bounds = button.getBoundingClientRect();
      const x = bounds.x + bounds.width / 2;
      const y = bounds.y + bounds.height / 2;
      const hit = document.elementFromPoint(x, y);
      return bounds.width > 0 && bounds.height > 0 && hit !== null && button.contains(hit)
        ? []
        : [button.getAttribute("aria-label")];
    }),
  );
  expect(inaccessible, "every complete-grid cell remains reachable and unobscured").toEqual([]);
  // Scrolling to a boundary must not silently widen the document either.
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
    width,
  );
}

for (const viewport of [
  { width: 320, height: 568 },
  { width: 390, height: 844 },
]) {
  test(`custom layout editor keeps complete geometry reachable at ${viewport.width}px`, async ({
    browser,
  }) => {
    const context = await browser.newContext({ viewport, hasTouch: true, reducedMotion: "reduce" });
    const page = await context.newPage();
    try {
      await page.goto(`./?${fixtureQuery()}`);
      await page.getByRole("button", { name: "New custom board", exact: true }).click();
      await editor(page)
        .getByRole("textbox", { name: "Layout name", exact: true })
        .fill("Phone layout");
      const size = editor(page).getByRole("spinbutton", { name: "Board size", exact: true });
      const save = editor(page).getByRole("button", { name: "Save layout", exact: true });
      await expect(size).toHaveValue("15");
      await fitsAndRemainsReachable(page, viewport.width, 225);

      // Touch paints a square boundary; keyboard paints its center.
      await editor(page).getByRole("radio", { name: "Triple word", exact: true }).check();
      await cell(page, 1, 1).tap();
      await expect(cell(page, 1, 1)).toHaveAccessibleName(/Triple word$/);
      await tabTo(page, grid(page).locator('[tabindex="0"]'));
      for (let step = 0; step < 7; step += 1) {
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowRight");
      }
      await expect(cell(page, 8, 8)).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(cell(page, 8, 8)).toHaveAccessibleName(/Triple word$/);

      await editor(page).getByRole("radio", { name: "Hexagon", exact: true }).check();
      await fitsAndRemainsReachable(page, viewport.width, 169);
      await expect(cell(page, 8, 8)).toHaveAccessibleName(/Triple word$/);
      // Top-right boundary of the regular hex must remain a real touch target.
      await cell(page, 1, 15).tap();
      await expect(cell(page, 1, 15)).toHaveAccessibleName(/Triple word$/);
      await size.fill("8");
      await expect(editor(page).getByRole("alert")).toBeVisible();
      await expect(save).toBeDisabled();
      await fitsAndRemainsReachable(page, viewport.width, 169);
      await editor(page).getByRole("radio", { name: "Square", exact: true }).check();
      await expect(save).toBeDisabled();
      await fitsAndRemainsReachable(page, viewport.width, 225);
      await expect(cell(page, 8, 8)).toHaveAccessibleName(/Triple word$/);
      await editor(page).getByRole("radio", { name: "Hexagon", exact: true }).check();
      await size.fill("15");
      await expect(editor(page).getByRole("alert")).toHaveCount(0);
      await expect(save).toBeEnabled();
      await fitsAndRemainsReachable(page, viewport.width, 169);
      await expect(cell(page, 8, 8)).toHaveAccessibleName(/Triple word$/);
      await expect(cell(page, 1, 15)).toHaveAccessibleName(/Triple word$/);
      // The retained hex boundary remains reachable through the keyboard registry.
      await tabTo(page, grid(page).locator('[tabindex="0"]'));
      await expect(cell(page, 8, 8)).toBeFocused();
      for (let step = 0; step < 7; step += 1) {
        await page.keyboard.press("ArrowRight");
      }
      for (let step = 0; step < 7; step += 1) {
        await page.keyboard.press("ArrowUp");
      }
      await expect(cell(page, 1, 15)).toBeFocused();
      await page.keyboard.press("Space");
      await expect(cell(page, 1, 15)).toHaveAccessibleName(/Triple word$/);
    } finally {
      await context.close();
    }
  });
}
