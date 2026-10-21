import { expect, test, type Page } from "@playwright/test";

import { freshOpening, gameFixture, selectOpening } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

const square = (phone: Page, row: number, column: number) =>
  phone
    .getByRole("region", { name: "Move editor", exact: true })
    .getByRole("button", { name: new RegExp(`^Row ${row}, column ${column}:`) });

/** Places the first available rack letters on the given squares by ordinary taps. */
async function draft(phone: Page, squares: [number, number][]) {
  const rack = phone.getByRole("list", { name: "Your rack", exact: true });
  for (const [row, column] of squares) {
    await rack
      .getByRole("button", { name: /^[A-Z], \d+ points$/ })
      .first()
      .click();
    await square(phone, row, column).click();
  }
}

test("waiting players prepare, persist and review a private pre-move; overlap clears it", async ({
  page,
  browser,
}) => {
  test.setTimeout(60_000);
  const phones = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const ada = await phones.newPage();
  const lin = await phones.newPage();
  const boardState = await observeSnapshots(page, "1", true);
  const adaState = await observeSnapshots(ada, "2");
  const linState = await observeSnapshots(lin, "3");
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  try {
    await page.goto(
      `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`,
    );
    // Its own deal: suites share one server, and each creation identity is single-use.
    await selectOpening(page, freshOpening(gameFixture.reconnectOpening.requestId));
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const href = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    for (const [phone, name] of [
      [ada, "Ada"],
      [lin, "Lin"],
    ] as const) {
      await phone.goto(href ?? "");
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
    await expect.poll(() => linState()?.own_rack.tiles.length).toBe(7);
    const linEditor = lin.getByRole("region", { name: "Move editor", exact: true });
    const linCommit = linEditor.getByRole("button", { name: "Commit move", exact: true });

    // 1. While Ada is active, Lin drafts over Ada's coming T square; Ada's move clears all of it.
    await draft(lin, [
      [8, 9],
      [9, 9],
    ]);
    await expect(linEditor.locator(".tentative-square")).toHaveCount(2);
    await expect(linEditor.getByRole("status").first()).toContainText("Private pre-move");
    await expect(linCommit).toBeDisabled();
    const adaEditor = ada.getByRole("region", { name: "Move editor", exact: true });
    const adaRack = ada.getByRole("list", { name: "Your rack", exact: true });
    for (const [index, letter] of ["A", "T"].entries()) {
      await adaRack
        .getByRole("button", { name: new RegExp(`^${letter},`) })
        .first()
        .click();
      await square(ada, 8, 8 + index).click();
    }
    // The waiting draft never reaches the server, Ada's phone or the shared board.
    expect(linState()?.own_preview).toBeUndefined();
    expect(adaState()?.own_preview?.player_id ?? "2").toBe("2");
    await expect(page.locator(".tentative-tile, .tentative-square")).toHaveCount(0);
    await adaEditor.getByRole("button", { name: "Commit move", exact: true }).click();
    await expect.poll(() => boardState()?.public.board.length).toBe(2);
    await expect(linEditor.locator(".tentative-square")).toHaveCount(0);
    await expect(linEditor.getByRole("status").first()).toContainText("whole pre-move was cleared");
    expect(
      await lin.evaluate(() =>
        Object.keys(localStorage).filter((key) => key.startsWith("scrabble:premove:")),
      ),
    ).toEqual([]);

    // 2. Lin passes; while Ada is active again Lin prepares a compatible pre-move.
    await linEditor.getByRole("button", { name: "Pass turn", exact: true }).click();
    await linEditor.getByRole("button", { name: "Confirm pass", exact: true }).click();
    await expect
      .poll(() => linState()?.public.phase)
      .toMatchObject({ kind: "playing", active_player: "2" });
    await draft(lin, [
      [9, 8],
      [10, 8],
    ]);
    await expect(linEditor.locator(".tentative-square")).toHaveCount(2);
    // Decision 1.54: a waiting pre-move gets a local, non-authoritative estimate, no hints.
    const estimate = linEditor.getByRole("region", { name: "Proposed move breakdown" });
    await expect(estimate).toContainText("Estimated score:");
    await expect(estimate).toContainText("Calculated on this device while you wait");
    const placed = await linEditor.locator(".tentative-square").allInnerTexts();
    // Same-device reload restores exactly that draft.
    await lin.reload();
    await expect(lin.getByRole("heading", { name: "Lin", exact: true })).toBeVisible();
    await expect(linEditor.locator(".tentative-square")).toHaveCount(2);
    expect(await linEditor.locator(".tentative-square").allInnerTexts()).toEqual(placed);
    await expect(linCommit).toBeDisabled();

    // 3. Ada's non-overlapping turn keeps it for Lin's explicit review; nothing auto-submits.
    const revision = boardState()?.public.revision;
    await ada.getByRole("button", { name: "Pass turn", exact: true }).click();
    await ada.getByRole("button", { name: "Confirm pass", exact: true }).click();
    await expect(lin.getByText("It is your turn.", { exact: true })).toBeVisible();
    await expect(linEditor.locator(".tentative-square")).toHaveCount(2);
    expect(await linEditor.locator(".tentative-square").allInnerTexts()).toEqual(placed);
    await page.waitForTimeout(500);
    expect(boardState()?.public.board.length).toBe(2);
    expect(boardState()?.public.revision).not.toBe(revision);
    expect(boardState()?.public.phase).toMatchObject({ kind: "playing", active_player: "3" });
    await expect(page.locator(".tentative-tile, .tentative-square")).toHaveCount(0);
  } finally {
    await phones.close();
  }
});
