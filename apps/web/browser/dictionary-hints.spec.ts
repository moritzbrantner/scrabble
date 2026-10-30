import { expect, test, type Browser, type Page } from "./fixtures";

import { gameFixture } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

async function startMatch(page: Page, browser: Browser, hints: boolean) {
  const phones = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const ada = await phones.newPage();
  const lin = await phones.newPage();
  const boardState = await observeSnapshots(page, "1", true);
  const adaState = await observeSnapshots(ada, "2");
  const linState = await observeSnapshots(lin, "3");
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  await page.goto(
    `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`,
  );
  const toggle = page.getByRole("checkbox", { name: /Dictionary hints/ });
  await expect(toggle).not.toBeChecked();
  if (hints) {
    await toggle.check();
  }
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
  return { phones, ada, boardState, adaState, linState };
}

test("only a hints-enabled match reveals dictionary membership before commit; both estimate", async ({
  page,
  browser,
  context,
}) => {
  test.setTimeout(90_000);
  const second = await context.newPage();
  const matches = [
    { hints: false, ...(await startMatch(page, browser, false)) },
    { hints: true, ...(await startMatch(second, browser, true)) },
  ];
  try {
    for (const { hints, ada, boardState, adaState, linState } of matches) {
      expect(boardState()?.public.configuration.dictionary_hints).toBe(hints ? true : undefined);
      const rack = ada.getByRole("list", { name: "Your rack", exact: true });
      const editor = ada.getByRole("region", { name: "Move editor", exact: true });
      for (const column of [8, 9]) {
        await rack
          .getByRole("button", { name: /^[A-Z]{1,3}, \d+ points$/ })
          .first()
          .click();
        await editor.getByRole("button", { name: new RegExp(`^Row 8, column ${column}:`) }).click();
      }
      await expect.poll(() => adaState()?.own_preview?.tiles.length).toBe(2);
      const evaluation = adaState()?.own_preview?.evaluation;
      const breakdown = editor.getByRole("region", { name: "Proposed move breakdown" });
      if (hints) {
        // Private membership feedback for the authenticated player only.
        if (evaluation?.status === "valid") {
          await expect(breakdown).toContainText("every word is in the match dictionary");
        } else {
          expect(evaluation).toMatchObject({ status: "invalid", problem: "invalid_words" });
          await expect(editor).toContainText("Invalid words:");
        }
      } else {
        // Hints off: an estimated score, never a membership verdict.
        expect(evaluation?.status).toBe("valid");
        await expect(breakdown).toContainText("Estimated score:");
        await expect(breakdown).toContainText("dictionary is checked when you commit");
        await expect(editor).not.toContainText("Invalid words:");
      }
      // No hint or draft reaches the other phone or the board.
      expect(linState()?.own_preview).toBeUndefined();
      expect(boardState()?.own_preview).toBeUndefined();
    }
  } finally {
    for (const match of matches) {
      await match.phones.close();
    }
  }
});
