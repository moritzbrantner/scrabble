import { test, expect } from "@playwright/test";
import data from "../src/fixtures/private-rack.json" with { type: "json" };
import { playerSnapshot, ruleset } from "../src/public-state";
const rules = ruleset.parse(data.ruleset);
const playing = playerSnapshot.parse(data.snapshots.playing);
const drawn = playerSnapshot.parse(data.snapshots.drawn);

test("blank editing and authoritative updates reconcile without changing a private rack", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("./?fixture=lobby");
  await page.evaluate(
    async ({ snapshot, rules }) => {
      const path = "/scrabble/browser/editor-fixture.tsx";
      const { showEditor } = (await import(path)) as typeof import("./editor-fixture");
      showEditor(snapshot, rules);
    },
    { snapshot: playing, rules },
  );
  const editor = page.getByRole("region", { name: "Move editor", exact: true });
  const blank = playing.own_rack.tiles.find((tile) => tile.face.kind === "blank");
  const letter = playing.own_rack.tiles.find((tile) => tile.face.kind === "letter");
  expect(blank).toBeDefined();
  expect(letter).toBeDefined();
  const blankButton = editor.locator(`[data-tile-id="${blank?.id}"]`).getByRole("button");
  await blankButton.click();
  await editor.getByRole("button", { name: "Row 8, column 8: center", exact: true }).click();
  const choices = editor.getByRole("group", { name: "Choose blank letter", exact: true });
  await expect(choices).toBeVisible();
  await expect(editor.getByRole("textbox")).toHaveCount(0);
  await expect(choices.getByRole("button").first()).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(choices).toHaveCount(0);
  await expect(blankButton).toBeFocused();
  await page.keyboard.press("Enter");
  await editor.getByRole("button", { name: "Row 8, column 8: center", exact: true }).press("Enter");
  await expect(choices.getByRole("button", { name: "A", exact: true })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(
    editor.getByRole("button", { name: "Row 8, column 8: tentative A", exact: true }),
  ).toBeFocused();
  const pass = editor.getByRole("button", { name: "Pass turn", exact: true });
  await pass.press("Enter");
  await expect(editor.getByRole("button", { name: "Confirm pass", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(editor.getByRole("group", { name: "Confirm pass", exact: true })).toHaveCount(0);
  await expect(pass).toBeFocused();
  await expect(
    editor.getByRole("button", { name: "Row 8, column 8: tentative A", exact: true }),
  ).toBeVisible();
  await expect(blankButton).toHaveAttribute("data-placed", "true");
  await editor.locator(`[data-tile-id="${letter?.id}"]`).getByRole("button").click();
  await editor.getByRole("button", { name: "Row 8, column 9: empty", exact: true }).click();
  await expect(editor.getByRole("status")).toContainText("Ready to commit");
  await blankButton.click();
  await editor.getByRole("button", { name: "Return selected tile", exact: true }).click();
  await expect(blankButton).toHaveAttribute("data-placed", "false");
  await expect(editor.locator(".tentative-square")).toHaveCount(1);
  await page.evaluate(
    async ({ snapshot, rules }) => {
      const path = "/scrabble/browser/editor-fixture.tsx";
      const { showEditor } = (await import(path)) as typeof import("./editor-fixture");
      showEditor(snapshot, rules);
    },
    { snapshot: drawn, rules },
  );
  await expect(editor.locator(".tentative-square")).toHaveCount(0);
  await expect(editor.locator(".committed-square")).toHaveCount(2);
  await expect(editor.getByRole("status")).toContainText("draft was cleared");
  const waiting = {
    ...drawn,
    public: { ...drawn.public, phase: { kind: "playing", active_player: "2", turn: "1" } },
  } satisfies typeof drawn;
  await page.evaluate(
    async ({ snapshot, rules }) => {
      const path = "/scrabble/browser/editor-fixture.tsx";
      const { showEditor } = (await import(path)) as typeof import("./editor-fixture");
      showEditor(snapshot, rules);
    },
    { snapshot: waiting, rules },
  );
  await expect(editor.getByRole("status")).toHaveText("Wait for your turn.");
  await expect(editor.getByRole("button", { name: "Commit move", exact: true })).toBeDisabled();
  for (const button of await editor
    .getByRole("list", { name: "Your rack", exact: true })
    .getByRole("button")
    .all()) {
    await expect(button).toBeDisabled();
  }
});
