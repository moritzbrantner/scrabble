// Independently authored #94 acceptance against 72cf2cd + the approved wire specification.
// Every HTTP request and browser snapshot reaches the real native server; no fake game/catalog.
import { expect, test, type Page, type Browser, type APIRequestContext } from "@playwright/test";
import { z } from "zod";

import { ruleset } from "../src/public-state";
import { gameFixture } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";
import { exchangeTwoOwnedTilesOnce } from "./exchange-wire-fault";

const identity = z.object({ name: z.string(), revision: z.string() });
const selection = z.object({
  wordList: identity,
  tile: identity,
  board: identity,
});
const settings = z.strictObject({
  rack_size: ruleset.shape.rack_size,
  bingo_bonus: ruleset.shape.bingo_bonus,
  exchange_minimum_bag: ruleset.shape.exchange_minimum_bag,
  scoreless_turn_limit: ruleset.shape.scoreless_turn_limit,
  minimum_players: ruleset.shape.minimum_players,
  maximum_players: ruleset.shape.maximum_players,
});
const catalogSchema = z.strictObject({
  version: z.literal(1),
  wordLists: z.array(z.strictObject({ identity, label: z.string() })),
  tileProfiles: z.array(
    z.strictObject({ identity, label: z.string(), tiles: ruleset.shape.tiles }),
  ),
  boardProfiles: z.array(
    z.strictObject({
      identity,
      label: z.string(),
      boardSize: z.number(),
      premiums: ruleset.shape.premiums,
    }),
  ),
  defaults: z.strictObject({
    selection,
    rules: settings,
    legacyCompatible: z.boolean(),
  }),
});
async function catalog(request: APIRequestContext) {
  const response = await request.get(`${gameFixture.api}/profiles`);
  expect(response.status(), "configured real server must expose the profile catalog").toBe(200);
  return catalogSchema.parse(await response.json());
}
function entry() {
  const url = new URL(gameFixture.endpoint);
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return `./?server=${encodeURIComponent(url.href)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`;
}
const word = (page: Page) => page.getByRole("combobox", { name: "Word list", exact: true });
const tiles = (page: Page) => page.getByRole("combobox", { name: "Tile profile", exact: true });
const board = (page: Page) => page.getByRole("combobox", { name: "Board layout", exact: true });
async function advanced(page: Page) {
  await page.getByText("Advanced match rules", { exact: true }).click();
}
async function guests(browser: Browser, host: Page) {
  const href = await host
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  expect(href).not.toBeNull();
  const contexts = await Promise.all([browser.newContext(), browser.newContext()]);
  const pages = await Promise.all(contexts.map((context) => context.newPage()));
  const readers = await Promise.all(
    pages.map((page, index) => observeSnapshots(page, String(index + 2))),
  );
  for (const [index, page] of pages.entries()) {
    await page.goto(href!);
    await page.getByLabel("Player name", { exact: true }).fill(index === 0 ? "Ada" : "Lin");
    await page.getByRole("button", { name: "Join game", exact: true }).click();
  }
  await host.getByRole("button", { name: "Start game", exact: true }).click();
  return { contexts, pages, readers };
}

test("untouched equivalent catalog defaults preserve exact legacy v2 creation", async ({
  page,
}) => {
  const bodies: unknown[] = [];
  page.on("request", (request) => {
    if (request.url() === `${gameFixture.api}/games` && request.method() === "POST") {
      bodies.push(request.postDataJSON());
    }
  });
  await page.goto(entry());
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  expect(bodies).toHaveLength(1);
  expect(z.object({ version: z.literal(2) }).parse(bodies[0]).version).toBe(2);
  expect(new Set(Object.keys(z.record(z.string(), z.unknown()).parse(bodies[0])))).toEqual(
    new Set(["requestId", "requestedAt", "version"]),
  );
});

test("three independent selectors and exact advanced values drive two simultaneous native matches", async ({
  page,
  browser,
  request,
}) => {
  test.setTimeout(90_000);
  const firstState = await observeSnapshots(page, "1", true);
  const secondContext = await browser.newContext();
  const second = await secondContext.newPage();
  const secondState = await observeSnapshots(second, "1", true);
  const owned: Awaited<ReturnType<typeof guests>>[] = [];
  try {
    await page.goto(entry());
    await expect(word(page)).toBeVisible();
    await expect(tiles(page)).toBeVisible();
    await expect(board(page)).toBeVisible();
    const sources = await catalog(request);
    expect(sources.defaults.legacyCompatible).toBe(true);
    const preview = page.getByRole("table", { name: "Board preview", exact: true });
    await expect(preview.locator("td")).toHaveCount(225);
    const originalTile = await tiles(page).inputValue();
    const originalBoard = await board(page).inputValue();
    await word(page).selectOption({ label: "Authored two-letter pairs" });
    expect(await tiles(page).inputValue()).toBe(originalTile);
    expect(await board(page).inputValue()).toBe(originalBoard);
    await tiles(page).selectOption({ label: "Double-value test tiles" });
    const selectedWord = await word(page).inputValue();
    await board(page).selectOption({ label: "Compact square 7" });
    await expect(preview.locator("td")).toHaveCount(49);
    await expect(preview).toHaveAttribute("data-board-size", "7");
    expect(await word(page).inputValue()).toBe(selectedWord);
    const chosen = [
      await word(page).inputValue(),
      await tiles(page).inputValue(),
      await board(page).inputValue(),
    ];
    await page.getByText("Settings", { exact: true }).click();
    await page.getByLabel("Interface language", { exact: true }).selectOption("de");
    await expect(page.locator("html")).toHaveAttribute("lang", "de");
    await page.getByLabel("Sprache der Oberfläche", { exact: true }).selectOption("en");
    expect([
      await word(page).inputValue(),
      await tiles(page).inputValue(),
      await board(page).inputValue(),
    ]).toEqual(chosen);
    await advanced(page);
    for (const [label, value] of [
      ["Rack size", "2"],
      ["Bingo bonus", "17"],
      ["Exchange minimum", "1000"],
      ["Scoreless-turn limit", "2"],
      ["Maximum players", "2"],
    ]) {
      await page.getByLabel(label!, { exact: true }).fill(value!);
    }
    await page.screenshot({
      path: test.info().outputPath("profile-chooser.png"),
    });
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
    await second.goto(entry());
    await word(second).selectOption({ label: "Authored two-letter pairs" });
    await advanced(second);
    await second.getByLabel("Rack size", { exact: true }).fill("3");
    await second.getByRole("button", { name: "Create game", exact: true }).click();
    await expect(second.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
    const first = await guests(browser, page);
    owned.push(first);
    const other = await guests(browser, second);
    owned.push(other);
    await expect.poll(() => first.readers[0]?.()?.own_rack.tiles.length).toBe(2);
    await expect.poll(() => other.readers[0]?.()?.own_rack.tiles.length).toBe(3);
    await expect.poll(() => firstState()?.public.configuration.board_size).toBe(7);
    await expect.poll(() => secondState()?.public.configuration.board_size).toBe(15);
    const initial = firstState();
    const initialPhone = first.readers[0]?.();
    const untouched = secondState();
    expect(initial).toBeDefined();
    expect(initialPhone).toBeDefined();
    expect(untouched).toBeDefined();
    if (!initial || !initialPhone || !untouched || !first.pages[0]) {
      throw new Error("Missing real wire projection");
    }
    expect(initial.public.game_id).not.toBe(untouched.public.game_id);
    expect(initial.public.dictionary).toEqual(untouched.public.dictionary);
    expect(initial.public.configuration.tiles).toEqual(
      sources.tileProfiles.find((row) => row.identity.name === "acceptance-valued-tiles")?.tiles,
    );
    expect(initial.public.configuration.premiums[24]).toBe("triple_word");
    expect(initial.public.configuration.bingo_bonus).toBe(17);
    expect(initial.public.configuration.maximum_players).toBe(2);
    const phone = first.pages[0];
    const editor = phone.getByRole("region", {
      name: "Move editor",
      exact: true,
    });
    let expectedPoints = 0;
    for (const [index, tile] of initialPhone.own_rack.tiles.entries()) {
      expectedPoints +=
        initial.public.configuration.tiles.find(
          (definition) =>
            definition.face.kind === tile.face.kind &&
            (definition.face.kind !== "letter" ||
              (tile.face.kind === "letter" && definition.face.letter === tile.face.letter)),
        )?.value ?? 0;
      await phone
        .getByRole("list", { name: "Your rack", exact: true })
        .locator(`[data-tile-id="${tile.id}"]`)
        .getByRole("button")
        .click();
      await editor
        .getByRole("button", {
          name: new RegExp(`^Row 4, column ${4 + index}:`),
        })
        .click();
      if (tile.face.kind === "blank") {
        await phone
          .getByRole("group", { name: "Choose blank letter", exact: true })
          .getByRole("button", { name: "A", exact: true })
          .click();
      }
    }
    await editor.getByRole("button", { name: "Commit move", exact: true }).click();
    await expect.poll(() => firstState()?.public.history.length).toBe(1);
    expect(firstState()?.public.players[0]?.score).toBe(expectedPoints * 3 + 17);
    expect(secondState()?.public.board).toEqual(untouched.public.board);
    expect(secondState()?.public.configuration).toEqual(untouched.public.configuration);
    expect(secondState()?.public.history).toEqual([]);
    await page.reload();
    await expect(page.getByRole("table", { name: "Scrabble board", exact: true })).toBeVisible();
    await expect.poll(() => firstState()?.public.configuration.board_size).toBe(7);
  } finally {
    for (const group of owned) {
      for (const context of group.contexts) {
        await context.close();
      }
    }
    await secondContext.close();
  }
});

test("invalid combinations preserve edits; restoring valid choices retries the same real v3 creation", async ({
  page,
  request,
}) => {
  test.setTimeout(60_000);
  await catalog(request);
  const hosted = async () =>
    z
      .object({ capacity: z.object({ hostedMatches: z.number() }) })
      .parse(await (await request.get(`${gameFixture.status}/status`)).json()).capacity
      .hostedMatches;
  const before = await hosted();
  await page.goto(entry());
  await word(page).selectOption({ label: "Authored unsupported Unicode" });
  await tiles(page).selectOption({ label: "Double-value test tiles" });
  await board(page).selectOption({ label: "Compact square 7" });
  await advanced(page);
  await page.getByLabel("Rack size", { exact: true }).fill("2");
  await page.getByLabel("Bingo bonus", { exact: true }).fill("17");
  const selected = [
    await word(page).inputValue(),
    await tiles(page).inputValue(),
    await board(page).inputValue(),
  ];
  const create = page.getByRole("button", { name: "Create game", exact: true });
  if (await create.isEnabled()) {
    await create.click();
  }
  await expect(page.getByRole("alert")).toBeVisible();
  expect(await hosted()).toBe(before);
  expect([
    await word(page).inputValue(),
    await tiles(page).inputValue(),
    await board(page).inputValue(),
  ]).toEqual(selected);
  await expect(page.getByLabel("Rack size", { exact: true })).toHaveValue("2");
  await expect(page.getByLabel("Bingo bonus", { exact: true })).toHaveValue("17");
  await word(page).selectOption({ label: "Authored two-letter pairs" });
  const bodies: string[] = [];
  await page.route(`${gameFixture.api}/games`, async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    bodies.push(route.request().postData() ?? "");
    if (bodies.length === 1) {
      const response = await route.fetch();
      expect(response.status()).toBe(200);
      await route.abort("failed");
    } else {
      await route.continue();
    }
  });
  await create.click();
  await expect(page.getByRole("alert")).toContainText("response was interrupted");
  await expect(page.getByLabel("Rack size", { exact: true })).toHaveValue("2");
  await page.getByRole("button", { name: "Retry creation", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  expect(bodies).toHaveLength(2);
  expect(bodies[0]).toBe(bodies[1]);
  const sent = z
    .object({ version: z.literal(3), selection, rules: settings })
    .parse(JSON.parse(bodies[0]!));
  expect(sent.rules.rack_size).toBe(2);
  expect(sent.rules.bingo_bonus).toBe(17);
  expect(await hosted()).toBe(before + 1);
});

test("an explicit rule edit restored to the same value still binds v3 source provenance", async ({
  page,
  request,
}) => {
  const sources = await catalog(request);
  const bodies: unknown[] = [];
  page.on("request", (request) => {
    if (request.url() === `${gameFixture.api}/games` && request.method() === "POST") {
      bodies.push(request.postDataJSON());
    }
  });
  await page.goto(entry());
  await advanced(page);
  const rack = page.getByLabel("Rack size", { exact: true });
  await rack.fill(String(sources.defaults.rules.rack_size + 1));
  await rack.fill(String(sources.defaults.rules.rack_size));
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  expect(bodies).toHaveLength(1);
  const sent = z.object({ version: z.literal(3), selection, rules: settings }).parse(bodies[0]);
  expect(sent.selection).toEqual(sources.defaults.selection);
  expect(sent.rules).toEqual(sources.defaults.rules);
});

test("untouched non-equivalent catalog defaults bind v3 and reach the real compact board", async ({
  page,
  browser,
  request,
}) => {
  test.setTimeout(60_000);
  test.skip(
    process.env.SCRABBLE_BROWSER_COMPACT_DEFAULT !== "1",
    "separate real compact-default fixture invocation",
  );
  const sources = await catalog(request);
  expect(sources.defaults.legacyCompatible).toBe(false);
  expect(sources.defaults.selection.board.name).toBe("acceptance-compact-board");
  const state = await observeSnapshots(page, "1", true);
  const bodies: unknown[] = [];
  page.on("request", (request) => {
    if (request.url() === `${gameFixture.api}/games` && request.method() === "POST") {
      bodies.push(request.postDataJSON());
    }
  });
  await page.goto(entry());
  await expect(board(page)).toBeVisible();
  await expect(board(page).locator("option:checked")).toHaveText("Compact square 7");
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  expect(bodies).toHaveLength(1);
  const sent = z.object({ version: z.literal(3), selection, rules: settings }).parse(bodies[0]);
  expect(sent.selection).toEqual(sources.defaults.selection);
  expect(sent.rules).toEqual(sources.defaults.rules);
  const group = await guests(browser, page);
  try {
    await expect.poll(() => state()?.public.configuration.board_size).toBe(7);
    await expect.poll(() => group.readers[0]?.()?.public.configuration.board_size).toBe(7);
    expect(state()?.public.configuration.premiums[24]).toBe("triple_word");
    expect(state()?.public.configuration.rack_size).toBe(sources.defaults.rules.rack_size);
  } finally {
    for (const context of group.contexts) {
      await context.close();
    }
  }
});

test("a real correlated exchange rejection restores phone actions and cancels its timeout", async ({
  page,
  browser,
  request,
}) => {
  test.setTimeout(60_000);
  test.skip(
    process.env.SCRABBLE_BROWSER_EXCHANGE_REJECTION !== "1",
    "separate low-bag native fixture invocation",
  );
  const sources = await catalog(request);
  expect(sources.tileProfiles.some((row) => row.identity.name === "acceptance-low-bag-tiles")).toBe(
    true,
  );
  const boardState = await observeSnapshots(page, "1", true);
  await page.goto(entry());
  await word(page).selectOption({ label: "Authored two-letter pairs" });
  await tiles(page).selectOption({ label: "Low-bag test tiles" });
  await advanced(page);
  await page.getByLabel("Rack size", { exact: true }).fill("13");
  await page.getByLabel("Maximum players", { exact: true }).fill("2");
  await page.getByLabel("Exchange minimum", { exact: true }).fill("1");
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  const group = await guests(browser, page);
  try {
    await expect.poll(() => group.readers[0]?.()?.own_rack.tiles.length).toBe(13);
    await expect.poll(() => group.readers[0]?.()?.public.remaining_tiles).toBe(1);
    const before = group.readers[0]?.();
    const phone = group.pages[0];
    if (!before || !phone || !before.own_rack.tiles[0] || !before.own_rack.tiles[1]) {
      throw new Error("missing actual low-bag native projection");
    }
    await exchangeTwoOwnedTilesOnce(phone, [
      before.own_rack.tiles[0].id,
      before.own_rack.tiles[1].id,
    ]);
    const editor = phone.getByRole("region", { name: "Move editor", exact: true });
    await editor.getByRole("button", { name: "Exchange tiles", exact: true }).click();
    await editor
      .locator(`[data-tile-id="${before.own_rack.tiles[0].id}"]`)
      .getByRole("button")
      .click();
    await editor.getByRole("button", { name: "Confirm exchange", exact: true }).click();
    await expect(
      phone.getByText("There are not enough tiles in the bag for this exchange.", { exact: true }),
    ).toBeVisible();
    const pass = editor.getByRole("button", { name: "Pass turn", exact: true });
    await expect(pass).toBeEnabled({ timeout: 1_000 });
    expect(group.readers[0]?.()?.own_rack).toEqual(before.own_rack);
    expect(group.readers[0]?.()?.public.phase).toEqual(before.public.phase);
    expect(boardState()?.public.history).toEqual([]);
    // Keep the real five-second timer boundary: an uncleared timer must not
    // overwrite already received authoritative feedback with a false timeout.
    await phone.waitForTimeout(5_200);
    await expect(editor.getByRole("status")).not.toContainText("Exchange was not confirmed");
    await expect(pass).toBeEnabled();
    await pass.click();
    await editor
      .getByRole("group", { name: "Confirm pass", exact: true })
      .getByRole("button", { name: "Confirm pass", exact: true })
      .click();
    await expect.poll(() => boardState()?.public.history.length).toBe(1);
  } finally {
    for (const context of group.contexts) {
      await context.close();
    }
  }
});

test("selected board geometry and premium rules are readable before creation", async ({
  page,
  request,
}) => {
  const sources = await catalog(request);
  await page.goto(entry());
  const summary = page.getByRole("group", { name: "Selected board", exact: true });
  for (const label of ["Compact square 7", "Classic square 15"]) {
    const selected = sources.boardProfiles.find((row) => row.label === label);
    if (!selected) {
      throw new Error("missing real authored board source");
    }
    await board(page).selectOption({ label });
    await expect(summary).toContainText(label);
    await expect(summary).toContainText(`${selected.boardSize} × ${selected.boardSize}`);
    await expect(summary).toContainText(/square/i);
    await expect(summary).toContainText(/all (?:new )?words|every (?:newly )?(?:formed )?word/i);
    for (const [premium, text] of [
      ["double_letter", "Double letter"],
      ["triple_letter", "Triple letter"],
      ["double_word", "Double word"],
      ["triple_word", "Triple word"],
    ] as const) {
      const count = selected.premiums.filter((cell) => cell === premium).length;
      await expect(summary.getByText(`${text}: ${count}`, { exact: true })).toBeVisible();
    }
    await expect(page.getByRole("button", { name: "Create game", exact: true })).toBeEnabled();
    await expect(page.getByRole("link", { name: "Join this game", exact: true })).toHaveCount(0);
  }
});

test("the match setup previews the board on narrow screens without horizontal overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(entry());
  const preview = page.getByRole("table", { name: "Board preview", exact: true });
  await expect(preview).toBeVisible();
  await expect(preview.locator("td")).toHaveCount(225);
  await page.getByText("Advanced match rules", { exact: true }).click();
  await expect(
    page.getByText("Each player can hold this many tiles on their rack.", { exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await page.screenshot({ path: test.info().outputPath("board-setup-mobile.png"), fullPage: true });
});
