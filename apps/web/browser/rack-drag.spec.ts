import { test, expect, type Page } from "./fixtures";

import data from "../src/fixtures/private-rack.json" with { type: "json" };
import { playerSnapshot, ruleset } from "../src/public-state";
import { mouseDrag, touchDrag } from "./drag";

const byId = (left: unknown, right: unknown) => String(left).localeCompare(String(right));

const rules = ruleset.parse(data.ruleset);
const playing = playerSnapshot.parse(data.snapshots.playing);
const blank = playing.own_rack.tiles.find((tile) => tile.face.kind === "blank");
const letters = playing.own_rack.tiles.filter((tile) => tile.face.kind === "letter");

async function showEditor(page: Page) {
  await page.goto("./?fixture=lobby");
  await page.evaluate(
    async ({ snapshot, rules }) => {
      const path = "/scrabble/browser/editor-fixture.tsx";
      const { showEditor } = (await import(path)) as typeof import("./editor-fixture");
      showEditor(snapshot, rules);
    },
    { snapshot: playing, rules },
  );
  return page.getByRole("region", { name: "Move editor", exact: true });
}

test("mouse drag reorders the rack and places, moves and returns draft tiles", async ({
  page,
}, testInfo) => {
  expect(blank).toBeDefined();
  expect(letters.length).toBeGreaterThanOrEqual(2);
  await page.setViewportSize({ width: 1280, height: 900 });
  const editor = await showEditor(page);
  const items = editor.locator(".phone-rack li[data-tile-id]");
  const order = () => items.evaluateAll((nodes) => nodes.map((node) => node.dataset["tileId"]));
  const initial = await order();
  const tile = (id: string | undefined) =>
    editor.locator(`.phone-rack li[data-tile-id="${id}"]`).getByRole("button");
  const square = (column: number, state: string) =>
    editor.getByRole("button", { name: new RegExp(`^Row 8, column ${column}: ${state},`) });

  // Rack → rack: physical rearrangement keeps identity and count.
  await mouseDrag(page, tile(initial[0]), tile(initial[2]));
  await expect.poll(order).toEqual([initial[1], initial[2], initial[0], ...initial.slice(3)]);
  expect([...(await order())].sort(byId)).toEqual([...initial].sort(byId));
  // Keyboard alternative.
  await tile(initial[0]).focus();
  await page.keyboard.press("Alt+ArrowLeft");
  await expect.poll(order).toEqual([initial[1], initial[0], initial[2], ...initial.slice(3)]);
  await expect(tile(initial[0])).toBeFocused();
  await expect(editor.getByRole("status").first()).toContainText("rack position 2");

  // Rack → board, board → board, board → rack.
  const letter = letters[0]!;
  await mouseDrag(page, tile(letter.id), square(8, "center"));
  await expect(editor.locator(".tentative-square")).toHaveCount(1);
  await expect(tile(letter.id)).toHaveAttribute("data-placed", "true");
  await mouseDrag(page, editor.locator(".tentative-square"), square(9, "empty"));
  await expect(square(8, "center")).toBeVisible();
  await expect(editor.locator(".tentative-square")).toHaveCount(1);
  await expect(editor.locator('[data-square-index="113"]')).toHaveClass(/tentative-square/);
  await mouseDrag(page, editor.locator(".tentative-square"), tile(initial[1]));
  await expect(editor.locator(".tentative-square")).toHaveCount(0);
  await expect(tile(letter.id)).toHaveAttribute("data-placed", "false");
  expect([...(await order())].sort(byId)).toEqual([...initial].sort(byId));

  // A blank asks for its letter once, then keeps it when repositioned.
  await mouseDrag(page, tile(blank?.id), square(8, "center"));
  const choices = editor.getByRole("group", { name: "Choose blank letter", exact: true });
  await choices.getByRole("button", { name: "A", exact: true }).click();
  await expect(square(8, "tentative A")).toBeVisible();
  await mouseDrag(page, square(8, "tentative A"), square(7, "empty"));
  await expect(choices).toHaveCount(0);
  await expect(square(7, "tentative A")).toBeVisible();
  // Tap composition still works alongside dragging.
  await tile(letters[1]!.id).click();
  await square(8, "center").click();
  await expect(editor.locator(".tentative-square")).toHaveCount(2);
  await testInfo.attach("rack-drag-desktop.png", {
    body: await page.screenshot(),
    contentType: "image/png",
  });
});

for (const viewport of [
  { width: 390, height: 844 },
  { width: 844, height: 390 },
]) {
  test(`touch drag places a tile and pointer cancel recovers at ${viewport.width}px`, async ({
    browser,
  }, testInfo) => {
    const context = await browser.newContext({ viewport, hasTouch: true, reducedMotion: "reduce" });
    const page = await context.newPage();
    try {
      const editor = await showEditor(page);
      const letter = letters[0]!;
      const tile = editor
        .locator(`.phone-rack li[data-tile-id="${letter.id}"]`)
        .getByRole("button");
      const centre = editor.getByRole("button", { name: /^Row 8, column 8: center,/ });
      await centre.scrollIntoViewIfNeeded();
      await tile.scrollIntoViewIfNeeded();
      await touchDrag(page, tile, centre, true);
      await expect(editor.getByRole("status").first()).toContainText("Drag cancelled");
      await expect(editor.locator(".tentative-square")).toHaveCount(0);
      await expect(tile).toHaveAttribute("data-placed", "false");
      await expect(editor.locator(".drag-ghost")).toBeHidden();
      await touchDrag(page, tile, centre);
      await expect(editor.locator(".tentative-square")).toHaveCount(1);
      await expect(tile).toHaveAttribute("data-placed", "true");
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
        viewport.width,
      );
      await testInfo.attach(`rack-drag-touch-${viewport.width}.png`, {
        body: await page.screenshot(),
        contentType: "image/png",
      });
    } finally {
      await context.close();
    }
  });
}
