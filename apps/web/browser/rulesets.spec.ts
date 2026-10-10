import { expect, test } from "@playwright/test";
import { z } from "zod";

import data from "../src/fixtures/ruleset-variant.json" with { type: "json" };
import { playerSnapshot, ruleset } from "../src/public-state";
const variant = z
  .strictObject({ ruleset, playing: playerSnapshot, committed: playerSnapshot })
  .parse(data);

test("a second native ruleset drives compact rack geometry, board size and Unicode blank choices", async ({
  browser,
}) => {
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
  });
  const page = await context.newPage();
  try {
    await page.goto("./?fixture=lobby");
    await page.evaluate(
      async ({ snapshot, rules }) => {
        const path = "/scrabble/browser/editor-fixture.tsx";
        const { showEditor } = (await import(path)) as typeof import("./editor-fixture");
        showEditor(snapshot, rules);
      },
      { snapshot: variant.playing, rules: variant.ruleset },
    );
    const editor = page.getByRole("region", { name: "Move editor", exact: true });
    const rack = editor.getByRole("list", { name: "Your rack", exact: true });
    await expect(rack.getByRole("button")).toHaveCount(3);
    await expect(editor.locator("[data-square-index]")).toHaveCount(49);
    const target = await rack.getByRole("button").first().boundingBox();
    expect(target?.height).toBeGreaterThanOrEqual(48);
    expect(target?.height).toBeLessThanOrEqual(72);
    await rack.getByRole("button", { name: "Blank tile, 0 points", exact: true }).click();
    await editor.getByRole("button", { name: "Row 4, column 4: center", exact: true }).click();
    const choices = editor.getByRole("group", { name: "Choose blank letter", exact: true });
    await expect(choices.getByRole("button", { name: "X", exact: true })).toHaveCount(0);
    await choices.getByRole("button", { name: "Ö", exact: true }).click();
    await rack.getByRole("button", { name: "L, 2 points", exact: true }).click();
    await editor.getByRole("button", { name: "Row 4, column 5: empty", exact: true }).click();
    await expect(editor.getByRole("button", { name: "Commit move", exact: true })).toBeEnabled();
    await page.evaluate(
      async ({ snapshot, rules }) => {
        const path = "/scrabble/browser/editor-fixture.tsx";
        const { showEditor } = (await import(path)) as typeof import("./editor-fixture");
        showEditor(snapshot, rules);
      },
      { snapshot: variant.committed, rules: variant.ruleset },
    );
    await expect(editor.locator(".committed-square")).toHaveCount(2);
    await expect(rack).toHaveAttribute("aria-disabled", "true");
    await expect(
      editor.getByRole("button", { name: "Row 4, column 4: committed Ö", exact: true }),
    ).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      390,
    );
  } finally {
    await context.close();
  }
});
