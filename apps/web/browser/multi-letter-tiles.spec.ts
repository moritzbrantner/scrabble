// Independently authored #98 acceptance (docs/specs/multi-letter-browser.md), written before the
// browser implementation against a60d72e. Every request reaches the real native fixture server;
// the multi-letter profile comes from its real catalog and the deal from its real factory key.
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";

import AxeBuilder from "@axe-core/playwright";
import {
  expect,
  test,
  type APIRequestContext,
  type Locator,
  type Page,
  type TestInfo,
} from "@playwright/test";
import { z } from "zod";

import { gameFixture, selectOpening } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

const SAMPLE_TILES = "acceptance-multi-letter-tiles";
const SAMPLE_WORDS = "acceptance-multi-letter-words";
const TILE_LABEL = "Multi-letter test tiles (CH, SCH)";
const WORD_LABEL = "Multi-letter test words";

// Deliberately loose: the browser's own schema is part of what #98 changes.
const looseFace = z.object({
  kind: z.string(),
  letter: z.string().optional(),
  letters: z.string().optional(),
});
const looseTiles = z.array(
  z.object({ face: looseFace, count: z.number(), value: z.number() }).loose(),
);
const looseCatalog = z
  .object({
    wordLists: z.array(z.object({ identity: z.object({ name: z.string() }).loose() }).loose()),
    tileProfiles: z.array(
      z.object({ identity: z.object({ name: z.string() }).loose(), tiles: looseTiles }).loose(),
    ),
  })
  .loose();
type Face = z.infer<typeof looseFace>;
type Tiles = z.infer<typeof looseTiles>;

/** The whole spelled face of a non-blank tile; "" for a blank. */
function spelled(face: unknown): string {
  const parsed: Face = looseFace.parse(face);
  return parsed.kind === "blank" ? "" : (parsed.letters ?? parsed.letter ?? "");
}
function valueOf(tiles: Tiles, face: string): number {
  const definition = tiles.find(
    (tile) => tile.face.kind !== "blank" && spelled(tile.face) === face,
  );
  if (definition === undefined) {
    throw new Error(`no configured face ${face}`);
  }
  return definition.value;
}

async function sampleCatalog(request: APIRequestContext) {
  const response = await request.get(`${gameFixture.api}/profiles`);
  expect(response.status()).toBe(200);
  const catalog = looseCatalog.parse(await response.json());
  const tiles = catalog.tileProfiles.find((row) => row.identity.name === SAMPLE_TILES);
  expect(
    tiles,
    "the real fixture catalog must offer the multi-letter sample tile profile (#98)",
  ).toBeDefined();
  expect(catalog.wordLists.some((row) => row.identity.name === SAMPLE_WORDS)).toBe(true);
  return tiles!.tiles;
}

function entry() {
  const url = new URL(gameFixture.endpoint);
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return `./?server=${encodeURIComponent(url.href)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`;
}
const wordList = (page: Page) => page.getByRole("combobox", { name: "Word list", exact: true });
const tileProfile = (page: Page) =>
  page.getByRole("combobox", { name: "Tile profile", exact: true });
const boardLayout = (page: Page) =>
  page.getByRole("combobox", { name: "Board layout", exact: true });

async function attach(testInfo: TestInfo, page: Page, name: string) {
  await testInfo.attach(name, {
    body: await page.screenshot({ fullPage: true, animations: "disabled" }),
    contentType: "image/png",
  });
}

/**
 * Every visible occurrence of `face` inside `scope` must sit completely inside its tile or
 * square (no overflow, clipping or ellipsis) at a legible size of at least 9 px. A long face
 * may be drawn smaller than a single letter, but never below that floor.
 */
async function expectFaceFits(scope: Locator, face: string, where: string) {
  const found = await scope.evaluate((root, face) => {
    const results: { inside: boolean; size: number; detail: string }[] = [];
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      const element = node.parentElement;
      if (node.textContent?.trim() !== face || element === null) {
        continue;
      }
      const tile = element.closest(
        ".rack-tile, .letter-tile, .phone-square, .tile-preview-square, button, td, li",
      );
      const range = document.createRange();
      range.selectNodeContents(node);
      const text = range.getBoundingClientRect();
      if (tile === null || text.width === 0 || text.height === 0) {
        continue;
      }
      const box = tile.getBoundingClientRect();
      results.push({
        inside:
          text.left >= box.left - 0.5 &&
          text.right <= box.right + 0.5 &&
          text.top >= box.top - 0.5 &&
          text.bottom <= box.bottom + 0.5,
        size: Number.parseFloat(getComputedStyle(element).fontSize),
        detail: `text ${JSON.stringify(text)} tile ${JSON.stringify(box)}`,
      });
    }
    return results;
  }, face);
  expect(found.length, `${where}: a visible ${face} face`).toBeGreaterThan(0);
  for (const result of found) {
    expect(result.inside, `${where}: ${face} fits its tile (${result.detail})`).toBe(true);
    expect(result.size, `${where}: ${face} is legible`).toBeGreaterThanOrEqual(9);
  }
}

test("the multi-letter sample is selected independently and its whole faces are listed", async ({
  page,
  request,
}, testInfo) => {
  const tiles = await sampleCatalog(request);
  expect(tiles.map((tile) => tile.face)).toEqual(
    expect.arrayContaining([
      { kind: "letters", letters: "CH" },
      { kind: "letters", letters: "SCH" },
      { kind: "letter", letter: "C" },
      { kind: "letter", letter: "H" },
      { kind: "blank" },
    ]),
  );
  await page.goto(entry());
  await expect(tileProfile(page), "the browser must accept a catalog with CH/SCH").toBeVisible();
  const original = {
    word: await wordList(page).inputValue(),
    tiles: await tileProfile(page).inputValue(),
    board: await boardLayout(page).inputValue(),
  };
  await tileProfile(page).selectOption({ label: TILE_LABEL });
  expect(await wordList(page).inputValue()).toBe(original.word);
  expect(await boardLayout(page).inputValue()).toBe(original.board);
  await wordList(page).selectOption({ label: WORD_LABEL });
  expect(await tileProfile(page).inputValue()).toBe(SAMPLE_TILES);
  expect(await boardLayout(page).inputValue()).toBe(original.board);

  await page.getByText("Tile distribution", { exact: true }).click();
  const distribution = page
    .locator("details")
    .filter({ has: page.getByText("Tile distribution", { exact: true }) });
  // Whole faces, never "□" or a truncated S/C for a multi-letter definition.
  await expect(distribution).toContainText(/(^|[^\p{L}])SCH([^\p{L}]|$)/u);
  await expect(distribution).toContainText(/(^|[^\p{L}])CH([^\p{L}]|$)/u);
  if ((await distribution.getByText("SCH", { exact: true }).count()) > 0) {
    await expectFaceFits(distribution, "SCH", "creation tile grid");
  }
  await attach(testInfo, page, "creation-tiles-desktop.png");
  await page.setViewportSize({ width: 390, height: 844 });
  if ((await distribution.getByText("SCH", { exact: true }).count()) > 0) {
    await expectFaceFits(distribution, "SCH", "mobile creation tile grid");
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await attach(testInfo, page, "creation-tiles-mobile.png");

  // The authoritative server, not the browser, rejects an incompatible pair without allocating.
  const hosted = async () =>
    z
      .object({ capacity: z.object({ hostedMatches: z.number() }) })
      .parse(await (await request.get(`${gameFixture.status}/status`)).json()).capacity
      .hostedMatches;
  const before = await hosted();
  await wordList(page).selectOption({ label: "Authored two-letter pairs" });
  const create = page.getByRole("button", { name: "Create game", exact: true });
  if (await create.isEnabled()) {
    await create.click();
  }
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toHaveCount(0);
  expect(await hosted()).toBe(before);
  expect(await tileProfile(page).inputValue()).toBe(SAMPLE_TILES);
});

test("a real match plays CH, A, T as CHAT and SCH, A through CH as SCHACH on whole-face tiles", async ({
  page,
  browser,
  request,
}, testInfo) => {
  test.setTimeout(120_000);
  const tiles = await sampleCatalog(request);
  const opening = z
    .strictObject({ requestId: z.string().regex(/^[0-9a-f]{32}$/), requestedAt: z.int() })
    .parse(
      JSON.parse(
        execFileSync(
          resolve(process.env.CARGO_TARGET_DIR ?? "target", "debug/examples/browser_game_fixture"),
          ["--multi-letter-opening", dirname(process.env.SCRABBLE_GAME_FIXTURE ?? "")],
          { encoding: "utf8", timeout: 30_000 },
        ),
      ),
    );
  const adaContext = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const linContext = await browser.newContext({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
    reducedMotion: "reduce",
  });
  const ada = await adaContext.newPage();
  const lin = await linContext.newPage();
  const boardState = await observeSnapshots(page, "1", true);
  const adaState = await observeSnapshots(ada, "2");
  const linState = await observeSnapshots(lin, "3");
  try {
    // Creation: explicit independent selections (v3) on the unchanged classic board.
    await page.goto(entry());
    await expect(tileProfile(page), "the browser must accept a catalog with CH/SCH").toBeVisible();
    await tileProfile(page).selectOption({ label: TILE_LABEL });
    await wordList(page).selectOption({ label: WORD_LABEL });
    await expect(boardLayout(page).locator("option:checked")).toHaveText("Classic square 15");
    await selectOpening(page, opening);
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const href = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    expect(href).not.toBeNull();
    for (const [phone, name] of [
      [ada, "Ada"],
      [lin, "Lin"],
    ] as const) {
      await phone.goto(href!);
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
    await expect.poll(() => linState()?.own_rack.tiles.length).toBe(7);
    const faces = (rack: { face: unknown }[] | undefined) =>
      (rack ?? []).map((tile) => looseFace.parse(tile.face));
    const adaFaces = faces(adaState()?.own_rack.tiles).map(spelled);
    const linFaces = faces(linState()?.own_rack.tiles).map(spelled);
    expect(adaFaces, "native opening gives Ada CH, A, T and a blank").toEqual(
      expect.arrayContaining(["CH", "A", "T", ""]),
    );
    expect(linFaces, "native opening gives Lin SCH and A").toEqual(
      expect.arrayContaining(["SCH", "A"]),
    );
    // The board's observed projection may trail the phones; wait for its started state.
    const total = tiles.reduce((sum, tile) => sum + tile.count, 0);
    await expect.poll(() => boardState()?.public.phase.kind).toBe("playing");
    await expect.poll(() => boardState()?.public.remaining_tiles).toBe(total - 14);
    const configuration = boardState()?.public.configuration;
    expect(configuration?.tiles).toEqual(tiles);
    if (configuration === undefined) {
      throw new Error("missing real configuration");
    }
    const ch = valueOf(tiles, "CH");
    const sch = valueOf(tiles, "SCH");

    // Private racks: one tile, whole face, its single value, an accessible name with both.
    const adaRack = ada.getByRole("list", { name: "Your rack", exact: true });
    const linRack = lin.getByRole("list", { name: "Your rack", exact: true });
    const chTile = adaRack.getByRole("button", { name: `CH, ${ch} points`, exact: true });
    await expect(chTile).toHaveCount(adaFaces.filter((face) => face === "CH").length);
    await expect(chTile.first().getByText("CH", { exact: true })).toBeVisible();
    await expect(chTile.first().getByText(String(ch), { exact: true })).toBeVisible();
    const schTile = linRack.getByRole("button", { name: `SCH, ${sch} points`, exact: true });
    await expect(schTile.first().getByText("SCH", { exact: true })).toBeVisible();
    await expect(adaRack.getByRole("listitem")).toHaveCount(7);
    await expect(linRack.getByRole("listitem")).toHaveCount(7);
    await expectFaceFits(adaRack, "CH", "desktop rack");
    await expectFaceFits(linRack, "SCH", "mobile rack");
    await attach(testInfo, lin, "mobile-rack-sch.png");

    // Keyboard placement on the desktop phone (actual tab order, arrows and Enter).
    const tabTo = async (phone: Page, target: Locator) => {
      for (let index = 0; index < 40; index++) {
        if (await target.evaluate((element) => element === document.activeElement)) {
          return;
        }
        await phone.keyboard.press("Tab");
      }
      throw new Error("Keyboard could not reach gameplay control");
    };
    const toSquare = async (phone: Page, row: number, column: number) => {
      const square = phone
        .getByRole("region", { name: "Move editor", exact: true })
        .locator("[data-square-index][tabindex='0']");
      await tabTo(phone, square);
      const index = Number(await square.getAttribute("data-square-index"));
      const [currentRow, currentColumn] = [Math.floor(index / 15), index % 15];
      for (let step = 0; step < Math.abs(currentRow - row); step++) {
        await phone.keyboard.press(currentRow > row ? "ArrowUp" : "ArrowDown");
      }
      for (let step = 0; step < Math.abs(currentColumn - column); step++) {
        await phone.keyboard.press(currentColumn > column ? "ArrowLeft" : "ArrowRight");
      }
      await phone.keyboard.press("Enter");
    };
    const placeByKeyboard = async (name: string, row: number, column: number) => {
      await tabTo(ada, adaRack.getByRole("button", { name, exact: true }).first());
      await ada.keyboard.press("Enter");
      await toSquare(ada, row, column);
    };
    const adaEditor = ada.getByRole("region", { name: "Move editor", exact: true });
    const commitByKeyboard = async () => {
      await tabTo(ada, adaEditor.getByRole("button", { name: "Commit move", exact: true }));
      await ada.keyboard.press("Enter");
    };

    // Blank assignment offers only single-letter faces, never CH or SCH.
    await placeByKeyboard("Blank tile, 0 points", 7, 10);
    const blankChoices = adaEditor.getByRole("group", { name: "Choose blank letter", exact: true });
    await expect(blankChoices).toBeVisible();
    const singles = tiles.flatMap((tile) =>
      tile.face.kind === "letter" ? [spelled(tile.face)] : [],
    );
    const offered = (await blankChoices.getByRole("button").allTextContents())
      .map((text) => text.trim())
      .filter((text) => text !== "Cancel blank choice");
    expect(offered).toEqual(singles);
    await expect(blankChoices.getByRole("button", { name: "CH", exact: true })).toHaveCount(0);
    await expect(blankChoices.getByRole("button", { name: "SCH", exact: true })).toHaveCount(0);
    await ada.keyboard.press("Escape");
    await expect(blankChoices).toHaveCount(0);
    await expect(adaEditor.locator(".tentative-square")).toHaveCount(0);

    // Private draft: CH occupies one square and keeps its whole face; the board stays committed-only.
    await placeByKeyboard(`CH, ${ch} points`, 7, 7);
    await placeByKeyboard(`A, ${valueOf(tiles, "A")} points`, 7, 8);
    await expect(
      adaEditor.getByRole("button", { name: /^Row 8, column 8: tentative CH,/ }),
    ).toBeVisible();
    await expect(adaEditor.locator(".tentative-square")).toHaveCount(2);
    await expectFaceFits(adaEditor.locator(".tentative-square").first(), "CH", "desktop draft");
    await expect(page.getByRole("cell", { name: /Tentative/ })).toHaveCount(0);
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
    await attach(testInfo, page, "desktop-board-preview-ch.png");
    await attach(testInfo, ada, "desktop-draft-ch.png");

    // CH + A spells CHA: rejected by the real dictionary, nothing canonical changes.
    const rejectedRacks = [adaState()?.own_rack, linState()?.own_rack];
    await commitByKeyboard();
    await expect(adaEditor.getByRole("status")).toContainText("Not in the dictionary: CHA.");
    expect(boardState()?.public.board).toEqual([]);
    expect(boardState()?.public.history).toEqual([]);
    expect([adaState()?.own_rack, linState()?.own_rack]).toEqual(rejectedRacks);
    await expect(page.getByRole("cell", { name: /: Committed / })).toHaveCount(0);

    // Adding T spells CHAT on three cells, scored with CH's value once.
    await placeByKeyboard(`T, ${valueOf(tiles, "T")} points`, 7, 9);
    await commitByKeyboard();
    const letterFactor: Record<string, number> = { double_letter: 2, triple_letter: 3 };
    const wordFactor: Record<string, number> = { double_word: 2, triple_word: 3 };
    const score = (cells: [string, number, number, boolean][]) => {
      let sum = 0;
      let factor = 1;
      for (const [face, row, column, fresh] of cells) {
        const premium = fresh ? (configuration.premiums[row * 15 + column] ?? "normal") : "normal";
        sum += valueOf(tiles, face) * (letterFactor[premium] ?? 1);
        factor *= wordFactor[premium] ?? 1;
      }
      return sum * factor;
    };
    const chat = score([
      ["CH", 7, 7, true],
      ["A", 7, 8, true],
      ["T", 7, 9, true],
    ]);
    const converge = async (turn: string) => {
      await expect
        .poll(() => {
          const board = boardState()?.public;
          return (
            board?.phase.kind === "playing" &&
            board.phase.turn === turn &&
            JSON.stringify(adaState()?.public) === JSON.stringify(board) &&
            JSON.stringify(linState()?.public) === JSON.stringify(board)
          );
        })
        .toBe(true);
    };
    await converge("1");
    const afterChat = boardState()?.public;
    expect(afterChat?.board.map((tile) => [tile.letter, tile.coordinate])).toEqual([
      ["CH", { row: 7, column: 7 }],
      ["A", { row: 7, column: 8 }],
      ["T", { row: 7, column: 9 }],
    ]);
    expect(afterChat?.history[0]?.action).toEqual({
      kind: "commit",
      words: ["CHAT"],
      move_score: chat,
      blank_count: 0,
    });
    expect(afterChat?.players.map((player) => player.score)).toEqual([chat, 0]);
    expect(afterChat?.remaining_tiles).toBe(total - 14 - 3);
    expect(afterChat?.players.map((player) => player.rack_count)).toEqual([7, 7]);
    await expect(
      page.getByRole("cell", { name: `Row 8, column 8: Committed CH, ${ch} points`, exact: true }),
    ).toBeVisible();
    await expect(page.locator(".letter-tile")).toHaveCount(3);
    await expect(page.locator(".letter-tile").getByText("CH", { exact: true })).toHaveCount(1);
    await expectFaceFits(page.locator(".letter-tile").first(), "CH", "committed board");
    const history = page.getByRole("region", { name: "Turn history", exact: true });
    await expect(history.getByRole("listitem").nth(0)).toContainText(
      `Turn 1: Ada — CHAT · ${chat} points`,
    );
    await expect(
      adaEditor.getByRole("button", { name: /^Row 8, column 8: committed CH,/ }),
    ).toBeVisible();
    await attach(testInfo, page, "desktop-board-chat.png");

    // Reconnect keeps the private multi-letter face and the committed one.
    const linRackIds = linState()?.own_rack.tiles.map((tile) => tile.id);
    await lin.reload();
    await expect.poll(() => linState()?.own_rack.tiles.map((tile) => tile.id)).toEqual(linRackIds);
    await expect(schTile.first()).toBeVisible();
    const linEditor = lin.getByRole("region", { name: "Move editor", exact: true });
    await expect(
      linEditor.getByRole("button", { name: /^Row 8, column 8: committed CH,/ }),
    ).toBeVisible();
    await expect(linEditor).toHaveAttribute("data-input", "touch");

    // Touch placement on the 390×844 phone: SCH and A above CH spell SCHACH through it.
    await schTile.first().tap();
    await linEditor.getByRole("button", { name: /^Row 6, column 8:/ }).tap();
    await linRack
      .getByRole("button", { name: `A, ${valueOf(tiles, "A")} points`, exact: true })
      .first()
      .tap();
    await linEditor.getByRole("button", { name: /^Row 7, column 8:/ }).tap();
    await expect(
      linEditor.getByRole("button", { name: /^Row 6, column 8: tentative SCH,/ }),
    ).toBeVisible();
    await expectFaceFits(
      linEditor.getByRole("button", { name: /^Row 6, column 8: tentative SCH,/ }),
      "SCH",
      "mobile draft",
    );
    await expect(page.getByRole("cell", { name: /Tentative/ })).toHaveCount(0);
    await attach(testInfo, lin, "mobile-draft-sch.png");
    await linEditor.getByRole("button", { name: "Commit move", exact: true }).tap();
    const schach = score([
      ["SCH", 5, 7, true],
      ["A", 6, 7, true],
      ["CH", 7, 7, false],
    ]);
    await converge("2");
    const afterSchach = boardState()?.public;
    expect(afterSchach?.history[1]?.action).toEqual({
      kind: "commit",
      words: ["SCHACH"],
      move_score: schach,
      blank_count: 0,
    });
    expect(afterSchach?.players.map((player) => player.score)).toEqual([chat, schach]);
    expect(afterSchach?.remaining_tiles).toBe(total - 14 - 3 - 2);
    await expect(
      page.getByRole("cell", {
        name: `Row 6, column 8: Committed SCH, ${sch} points`,
        exact: true,
      }),
    ).toBeVisible();
    await expect(history.getByRole("listitem").nth(1)).toContainText(
      `Turn 2: Lin — SCHACH · ${schach} points`,
    );
    await expectFaceFits(
      linEditor.getByRole("button", { name: /^Row 6, column 8: committed SCH,/ }),
      "SCH",
      "mobile committed square",
    );
    await expectFaceFits(page.locator("td").filter({ hasText: "SCH" }), "SCH", "committed board");
    await attach(testInfo, lin, "mobile-board-schach.png");
    await attach(testInfo, page, "desktop-board-schach.png");
    expect(await lin.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
    for (const target of [page, ada, lin]) {
      expect((await new AxeBuilder({ page: target }).analyze()).violations).toEqual([]);
    }

    // The shared board at phone width keeps SCH legible inside its cell.
    await page.setViewportSize({ width: 390, height: 844 });
    await expectFaceFits(page.locator("td").filter({ hasText: "SCH" }), "SCH", "mobile board");
    await attach(testInfo, page, "mobile-board-schach.png");

    // Complete the match by passes; final deductions count each rack tile's value once.
    const limit = configuration.scoreless_turn_limit;
    for (let index = 0; index < limit; index++) {
      const phone = index % 2 === 0 ? ada : lin;
      await phone.getByRole("button", { name: "Pass turn", exact: true }).click();
      await phone
        .getByRole("group", { name: "Confirm pass", exact: true })
        .getByRole("button", { name: "Confirm pass", exact: true })
        .click();
      if (index < limit - 1) {
        await converge(String(3 + index));
      }
    }
    await expect.poll(() => boardState()?.public.phase.kind).toBe("finished");
    const rackValue = (rack: { face: unknown }[] | undefined) =>
      (rack ?? []).reduce(
        (sum, tile) => sum + (spelled(tile.face) === "" ? 0 : valueOf(tiles, spelled(tile.face))),
        0,
      );
    expect(boardState()?.public.players.map((player) => player.score)).toEqual([
      chat - rackValue(adaState()?.own_rack.tiles),
      schach - rackValue(linState()?.own_rack.tiles),
    ]);
    const finalHistory = boardState()?.public.history;
    await page.reload();
    await expect.poll(() => boardState()?.public.history).toEqual(finalHistory);
    await expect(
      page.getByRole("cell", { name: `Row 8, column 8: Committed CH, ${ch} points`, exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("cell", {
        name: `Row 6, column 8: Committed SCH, ${sch} points`,
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("region", { name: "Turn history", exact: true }).getByRole("listitem").nth(1),
    ).toContainText(`SCHACH · ${schach} points`);
  } finally {
    await adaContext.close();
    await linContext.close();
  }
});
