import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import data from "../src/fixtures/private-rack.json" with { type: "json" };
import { playerSnapshot, ruleset } from "../src/public-state";
import { gameFixture } from "./game-fixture";
const rules = ruleset.parse(data.ruleset);
const playing = playerSnapshot.parse(data.snapshots.playing);

async function accessible(page: Page) {
  const result = await new AxeBuilder({ page }).analyze();
  expect(result.violations).toEqual([]);
}
async function editor(page: Page) {
  await page.goto("./?fixture=lobby");
  await page.evaluate(
    async ({ snapshot, rules }) => {
      const path = "/scrabble/browser/editor-fixture.tsx";
      const { showEditor } = (await import(path)) as typeof import("./editor-fixture");
      showEditor(snapshot, rules);
    },
    { snapshot: playing, rules },
  );
  await expect(page.getByRole("region", { name: "Move editor", exact: true })).toBeVisible();
}

test("axe covers the shared board in every phase and the active controller", async ({ page }) => {
  for (const phase of ["lobby", "playing", "finished"]) {
    await page.goto(`./?fixture=${phase}`);
    await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
    await accessible(page);
  }
  await editor(page);
  await accessible(page);
});

test("axe covers real creation, lobby, player join, and waiting controller", async ({
  page,
  browser,
}) => {
  const query = new URLSearchParams({
    server: gameFixture.endpoint,
    api: gameFixture.api,
    certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
  });
  await page.goto(`./?${query}`);
  await expect(page.getByRole("button", { name: "Create game", exact: true })).toBeVisible();
  await accessible(page);
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("button", { name: "Start game", exact: true })).toBeVisible();
  await accessible(page);
  const href = await page
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  if (href === null) {
    throw new Error("No invite");
  }
  const phoneContext = await browser.newContext();
  const phone = await phoneContext.newPage();
  try {
    await phone.goto(href);
    await expect(phone.getByLabel("Player name", { exact: true })).toBeVisible();
    await accessible(phone);
    await phone.getByLabel("Player name", { exact: true }).fill("Ada");
    await phone.getByRole("button", { name: "Join game", exact: true }).click();
    await expect(phone.getByText("Waiting for the host to start.", { exact: true })).toBeVisible();
    await accessible(phone);
  } finally {
    await phoneContext.close();
  }
});

for (const viewport of [
  { width: 390, height: 844 },
  { width: 844, height: 390 },
  { width: 320, height: 568 },
]) {
  test(`touch controller ${viewport.width}x${viewport.height} cancels gestures without losing the draft`, async ({
    browser,
  }) => {
    const context = await browser.newContext({ viewport, hasTouch: true, reducedMotion: "reduce" });
    const page = await context.newPage();
    try {
      await editor(page);
      const region = page.getByRole("region", { name: "Move editor", exact: true });
      await expect(region).toHaveAttribute("data-input", "touch");
      const rack = region.getByRole("list", { name: "Your rack", exact: true });
      const button = rack.getByRole("button").first();
      const bounds = await button.boundingBox();
      expect(bounds?.width).toBeGreaterThanOrEqual(48);
      expect(bounds?.height).toBeGreaterThanOrEqual(48);
      const letter = playing.own_rack.tiles.find((tile) => tile.face.kind === "letter");
      if (letter === undefined) {
        throw new Error("No fixture letter");
      }
      await rack.locator(`[data-tile-id="${letter.id}"]`).getByRole("button").tap();
      await region.getByRole("button", { name: "Row 8, column 8: center", exact: true }).tap();
      await expect(region.locator(".tentative-square")).toHaveCount(1);
      await button.tap();
      await expect(button).toHaveAttribute("aria-pressed", "true");
      await region.dispatchEvent("pointercancel", { pointerType: "touch" });
      await expect(button).toHaveAttribute("aria-pressed", "false");
      await expect(region.locator(".tentative-square")).toHaveCount(1);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
        viewport.width,
      );
      const commit = region.getByRole("button", { name: "Commit move", exact: true });
      const action = await commit.boundingBox();
      expect(action?.y).toBeLessThan(viewport.height);
      await expect(region).toHaveAttribute("data-input", "touch");
      for (const name of ["Commit move", "Pass turn", "Exchange tiles"]) {
        const bounds = await region.getByRole("button", { name, exact: true }).boundingBox();
        if (bounds === null) {
          throw new Error("Missing action bounds");
        }
        expect(bounds.y + bounds.height).toBeLessThanOrEqual(viewport.height);
      }
      await expect(page.locator("main.player-phone")).toHaveScreenshot(
        `phone-touch-${viewport.width}x${viewport.height}.png`,
        { animations: "disabled", maxDiffPixelRatio: 0.005 },
      );
      await accessible(page);
    } finally {
      await context.close();
    }
  });
}

test("keyboard enters the board once, navigates squares, and leaves to turn actions", async ({
  page,
}) => {
  await editor(page);
  const region = page.getByRole("region", { name: "Move editor", exact: true });
  const rack = region.getByRole("list", { name: "Your rack", exact: true });
  await page.keyboard.press("Tab");
  const first = rack.getByRole("button").first();
  await expect(first).toBeFocused();
  await page.keyboard.press("Enter");
  for (let index = 0; index < playing.own_rack.tiles.length; index++) {
    await page.keyboard.press("Tab");
  }
  const center = region.getByRole("button", { name: "Row 8, column 8: center", exact: true });
  await expect(center).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(
    region.getByRole("button", { name: "Row 8, column 9: empty", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(center).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(first).toHaveAttribute("aria-pressed", "false");
  await expect(first).toBeFocused();
});

test("board command help is keyboard discoverable", async ({ page }) => {
  await editor(page);
  const help = page.locator(".keyboard-help summary");
  await help.focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".keyboard-help dl")).toBeVisible();
  for (const key of ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "Escape"]) {
    await expect(page.getByText(key, { exact: true })).toBeVisible();
  }
});
