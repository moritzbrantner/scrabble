import { expect, test, type Browser, type Page } from "@playwright/test";

import { gameFixture, selectOpening } from "../browser/game-fixture";
import { observeSnapshots } from "../browser/pages-observer";

/**
 * Review scenario `two-player-turn` (#185): a fresh disposable native match with a
 * shared board, Ada and Lin in separate browser contexts, and Ada's legitimate
 * proposed A/T placement waiting to be committed. With `--interactive` the browsers
 * stay open for manual takeover until the reviewer resumes or closes the inspector.
 */
const interactive = process.env.SCRABBLE_REVIEW_INTERACTIVE === "1";

const square = (phone: Page, row: number, column: number) =>
  phone
    .getByRole("region", { name: "Move editor", exact: true })
    .getByRole("button", { name: new RegExp(`^Row ${row}, column ${column}:`) });

async function phone(browser: Browser) {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
  return { context, page: await context.newPage() };
}

test("two-player-turn", async ({ page, browser }, testInfo) => {
  if (interactive) {
    test.setTimeout(0);
  }
  const ada = await phone(browser);
  const lin = await phone(browser);
  const boardState = await observeSnapshots(page, "1", true);
  const adaState = await observeSnapshots(ada.page, "2");
  const linState = await observeSnapshots(lin.page, "3");
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  try {
    await page.goto(
      `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`,
    );
    // The native fixture derives an opening in which Ada, moving first, can play AT.
    await selectOpening(page);
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const href = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    for (const [player, name] of [
      [ada.page, "Ada"],
      [lin.page, "Lin"],
    ] as const) {
      await player.goto(href ?? "");
      await player.getByLabel("Player name", { exact: true }).fill(name);
      await player.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(player.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
    await expect.poll(() => linState()?.own_rack.tiles.length).toBe(7);

    const adaRack = ada.page.getByRole("list", { name: "Your rack", exact: true });
    for (const [index, letter] of ["A", "T"].entries()) {
      await adaRack
        .getByRole("button", { name: new RegExp(`^${letter},`) })
        .first()
        .click();
      await square(ada.page, 8, 8 + index).click();
    }

    // Smoke assertion: the authoritative state the reviewer takes over.
    const adaEditor = ada.page.getByRole("region", { name: "Move editor", exact: true });
    await expect(adaEditor.locator(".tentative-square")).toHaveCount(2);
    await expect.poll(() => adaState()?.own_preview?.tiles.length).toBe(2);
    await expect(adaEditor.getByRole("button", { name: "Commit move", exact: true })).toBeEnabled();
    expect(boardState()?.public.phase).toMatchObject({ kind: "playing", active_player: "2" });
    expect(boardState()?.public.board.length).toBe(0);
    expect(linState()?.own_preview).toBeUndefined();
    await expect(page.locator(".tentative-tile, .tentative-square")).toHaveCount(0);

    for (const [name, view] of [
      ["board", page],
      ["ada", ada.page],
      ["lin", lin.page],
    ] as const) {
      const path = testInfo.outputPath(`${name}.png`);
      await view.screenshot({ path, fullPage: true });
      await testInfo.attach(name, { path, contentType: "image/png" });
    }
    if (interactive) {
      console.log(
        "Review ready: shared board, Ada and Lin are live. Play on in any window; resume or close the Playwright Inspector to finish and clean up.",
      );
      await page.pause();
    }
  } finally {
    await ada.context.close();
    await lin.context.close();
  }
});
