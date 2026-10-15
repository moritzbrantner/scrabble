// Independent #96 integration acceptance. Current native hex correctness is
// reused; this file checks actual catalog/UI/server exposure, never a fake game.
import { expect, test, type Locator } from "@playwright/test";

import { gameFixture } from "./game-fixture";

const query = new URLSearchParams({
  server: gameFixture.endpoint,
  api: gameFixture.api,
  certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
});

async function previewCells(preview: Locator) {
  return preview.getByRole("cell").or(preview.getByRole("gridcell"));
}

for (const width of [1280, 390]) {
  test(`real hex sample boards preserve independent choices and preview on ${width}px`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`./?${query}`);
    const wordList = page.getByRole("combobox", { name: "Word list", exact: true });
    const tileProfile = page.getByRole("combobox", { name: "Tile profile", exact: true });
    const boardLayout = page.getByRole("combobox", { name: "Board layout", exact: true });
    await expect(boardLayout).toBeVisible();
    const originalWord = await wordList.inputValue();
    const originalTiles = await tileProfile.inputValue();
    const hexOptions = boardLayout.getByRole("option").filter({ hasText: /hex/i });
    expect(
      await hexOptions.count(),
      "the real catalog offers independently selectable hex sample configurations for both policies",
    ).toBeGreaterThanOrEqual(2);
    const candidates = await hexOptions.evaluateAll((elements) =>
      elements.map((element) => ({
        value: (element as HTMLOptionElement).value,
        label: element.textContent ?? "",
      })),
    );
    // Do not prescribe exact profile names or wording. Both existing policies
    // must be discoverable as independent board choices rather than tile swaps.
    expect(candidates.some((entry) => /main.*word/i.test(entry.label))).toBe(true);
    expect(candidates.some((entry) => /all.*word/i.test(entry.label))).toBe(true);
    for (const candidate of candidates) {
      await boardLayout.selectOption(candidate.value);
      expect(await wordList.inputValue()).toBe(originalWord);
      expect(await tileProfile.inputValue()).toBe(originalTiles);
      const preview = page
        .getByRole("table", { name: "Board preview", exact: true })
        .or(page.getByRole("grid", { name: "Board preview", exact: true }));
      await expect(preview).toBeVisible();
      const side = Number(await preview.getAttribute("data-board-size"));
      expect(side % 2).toBe(1);
      expect(side).toBeGreaterThanOrEqual(3);
      expect(side).toBeLessThanOrEqual(15);
      const radius = Math.floor(side / 2);
      const cells = await previewCells(preview);
      await expect(cells).toHaveCount(3 * radius * radius + 3 * radius + 1);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
        width,
      );
      await testInfo.attach(`hex-choice-${width}.png`, {
        body: await page.screenshot(),
        contentType: "image/png",
      });
    }
  });
}
