// Guest-owned private tile profiles (#100; Decisions 1.25, 1.27, 1.29, 1.45) against the
// real native fixture server. The create page has region "Custom tile profiles" with
// "New tile profile" (a copy of the selected tile profile), a saved list with
// "Edit <name>" / "Delete <name>" (confirmed by "Delete <name>?"), and the editor region
// "Tile profile editor" with a "Tiles" table of Face/Count/Points rows plus "Blank tiles".
import { expect, test, type Page } from "@playwright/test";

import { gameFixture } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

const query = new URLSearchParams({
  server: gameFixture.endpoint,
  api: gameFixture.api,
  certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
});
const library = (page: Page) =>
  page.getByRole("region", { name: "Custom tile profiles", exact: true });
const editor = (page: Page) =>
  page.getByRole("region", { name: "Tile profile editor", exact: true });

test("a guest copies, edits, saves, plays and deletes a private tile profile", async ({
  page,
}, testInfo) => {
  test.setTimeout(120_000);
  const name = `Tens ${testInfo.workerIndex}-${Date.now()}`;
  const board = await observeSnapshots(page, "1", true);
  await page.goto(`./?${query}`);
  await expect(library(page)).toBeVisible({ timeout: 10_000 });
  await library(page).getByRole("button", { name: "New tile profile", exact: true }).click();
  const form = editor(page);
  await expect(form.getByRole("textbox", { name: "Profile name", exact: true })).toHaveValue(
    /^Copy of /,
  );
  await form.getByRole("textbox", { name: "Profile name", exact: true }).fill(name);
  const face = form.getByRole("textbox", { name: "Face, row 1", exact: true });
  const firstFace = await face.inputValue();
  expect(firstFace).toMatch(/^\p{Lu}+$/u);

  // Client feedback names the row; the server's reason is shown for its own rules.
  await face.fill("");
  await expect(form.getByRole("alert")).toContainText("Row 1 needs a face");
  await expect(form.getByRole("button", { name: "Save profile", exact: true })).toBeDisabled();
  const secondFace = await form
    .getByRole("textbox", { name: "Face, row 2", exact: true })
    .inputValue();
  await face.fill(secondFace);
  await form.getByRole("button", { name: "Save profile", exact: true }).click();
  await expect(form.getByRole("alert")).toContainText("The server rejected this profile:");
  expect(
    await page.evaluate(() =>
      Object.keys(localStorage).some((key) => key.startsWith("scrabble:tile-profile-owner:")),
    ),
  ).toBe(false);

  await face.fill(firstFace);
  await form.getByRole("spinbutton", { name: "Points, row 1", exact: true }).fill("10");
  await form.getByRole("button", { name: "Add tile", exact: true }).click();
  const added = await form.getByRole("textbox", { name: /^Face, row \d+$/ }).count();
  await form.getByRole("textbox", { name: `Face, row ${added}`, exact: true }).fill("ch");
  await form.getByRole("spinbutton", { name: "Blank tiles", exact: true }).fill("1");
  await form.getByRole("button", { name: "Save profile", exact: true }).click();
  await expect(form.getByRole("status").filter({ hasText: "Profile saved." })).toBeVisible();

  // It is selectable for a real authoritative match, independently of words and board.
  const tiles = page.getByRole("combobox", { name: "Tile profile", exact: true });
  await tiles.selectOption({ label: name });
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  await expect.poll(() => board()?.public.configuration.tiles.length).toBeGreaterThan(0);
  const used = board()!.public.configuration.tiles;
  expect(
    used.find((tile) => tile.face.kind === "letter" && tile.face.letter === firstFace)?.value,
  ).toBe(10);
  expect(used.some((tile) => tile.face.kind === "letters" && tile.face.letters === "CH")).toBe(
    true,
  );
  expect(used.find((tile) => tile.face.kind === "blank")?.count).toBe(1);

  // It survives a new session and is deleted only after confirmation.
  await page.goto(`./?${query}`);
  const saved = library(page).getByRole("listitem").filter({ hasText: name });
  await expect(saved).toHaveCount(1);
  const asked: string[] = [];
  page.once("dialog", (dialog) => {
    asked.push(dialog.message());
    void dialog.dismiss();
  });
  await saved.getByRole("button", { name: `Delete ${name}`, exact: true }).click();
  await expect.poll(() => asked).toEqual([`Delete ${name}?`]);
  await expect(saved).toHaveCount(1);
  page.once("dialog", (dialog) => void dialog.accept());
  await saved.getByRole("button", { name: `Delete ${name}`, exact: true }).click();
  await expect(
    library(page).getByRole("status").filter({ hasText: "Tile profile deleted." }),
  ).toBeVisible();
  await expect(saved).toHaveCount(0);
  await expect(tiles.locator("option", { hasText: name })).toHaveCount(0);
});
