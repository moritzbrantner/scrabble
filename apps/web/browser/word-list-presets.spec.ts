// Explicit acceptance against operator-prepared real corpora, never a mock catalog.
import { expect, test } from "@playwright/test";
import { z } from "zod";

import { profileCatalog } from "../src/profile-model";
import { gameFixture } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

test("prepared ENABLE2K is an independent selection and survives match reload", async ({
  page,
  request,
}) => {
  test.skip(!process.env.SCRABBLE_PRESET_ACCEPTANCE_ROOT, "requires prepared real corpora");
  test.setTimeout(60_000);
  // Metadata precedes hosting::serve's corpus load. Wait for the real service,
  // rather than racing its startup with the browser's first catalog request.
  await expect
    .poll(
      async () => {
        try {
          return (await request.get(`${gameFixture.api}/profiles/hex`)).status();
        } catch {
          return 0;
        }
      },
      { timeout: 30_000 },
    )
    .toBe(200);
  const sources = profileCatalog.parse(
    await (await request.get(`${gameFixture.api}/profiles/hex`)).json(),
  );
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  const state = await observeSnapshots(page, "1", true);
  const creationBodies: unknown[] = [];
  page.on("request", (request) => {
    if (request.url() === `${gameFixture.api}/games` && request.method() === "POST") {
      creationBodies.push(request.postDataJSON());
    }
  });
  await page.goto(
    `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`,
  );
  const words = page.getByRole("combobox", { name: "Word list", exact: true });
  const tiles = page.getByRole("combobox", { name: "Tile profile", exact: true });
  const board = page.getByRole("combobox", { name: "Board layout", exact: true });
  await expect(words).toHaveValue("scowl-en-us-60");
  await expect(words.locator("option:checked")).toContainText("English (US)");
  const originalTiles = await tiles.inputValue();
  const originalBoard = await board.inputValue();
  await words.selectOption("scowl-en-gb-60");
  await expect(words.locator("option:checked")).toContainText("English (UK)");
  await words.selectOption("enable2k-en");
  await expect(words.locator("option:checked")).toContainText("ENABLE2K 2000, public domain");
  await expect(words.locator("option:checked")).toContainText("not official tournament lexicon");
  await expect(tiles).toHaveValue(originalTiles);
  await expect(board).toHaveValue(originalBoard);
  await page.screenshot({
    path: test.info().outputPath("enable2k-selection-desktop.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(words).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await page.screenshot({
    path: test.info().outputPath("enable2k-selection-mobile.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  expect(creationBodies).toHaveLength(1);
  const body = z
    .object({
      version: z.literal(3),
      selection: z.object({ wordList: z.object({ name: z.string(), revision: z.string() }) }),
    })
    .parse(creationBodies[0]);
  expect(body.selection.wordList.name).toBe("enable2k-en");
  expect(body.selection.wordList).toEqual(
    sources.wordLists.find((row) => row.identity.name === "enable2k-en")?.identity,
  );
  await expect.poll(() => state()?.public.dictionary.name).toBe("enable2k-en");
  expect(state()?.public.dictionary).toEqual(body.selection.wordList);
  const configuration = state()?.public.configuration;
  expect(configuration).toBeDefined();
  await page.reload();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  await expect.poll(() => state()?.public.configuration).toEqual(configuration);
  await page.screenshot({
    path: test.info().outputPath("enable2k-match-mobile.png"),
    fullPage: true,
  });
});
