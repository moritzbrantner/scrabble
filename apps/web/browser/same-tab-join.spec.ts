import { expect, test } from "./fixtures";
import { fixtureQuery } from "./game-fixture";
import { storyTests } from "./stories/story-test";
import { openTable, readInvite, roster } from "./stories/match-table";

storyTests("SAME-TAB-1", {
  open: async ({ page, browser }) => ({
    page,
    context: await browser.newContext(),
    invite: "",
    boardUrl: "",
    playerId: "",
    boardId: "",
  }),
  close: async (world) => world.context.close(),
  initial: async ({ page }) => {
    await page.goto(`./?${fixtureQuery()}`);
  },
  events: {
    CREATE: async (world) => {
      const { page } = world;
      await page.getByRole("button", { name: "Create game", exact: true }).click();
      world.invite = await readInvite(page);
      world.boardUrl = page.url();
      world.boardId = await page.evaluate(() => {
        const key = Object.keys(sessionStorage).find((key) => key.startsWith("scrabble:board:v1:"));
        return key ? String(JSON.parse(sessionStorage.getItem(key)!).playerId) : "";
      });
    },
    JOIN_AS_PLAYER: async ({ page }) => {
      await page.getByRole("link", { name: "Join as player", exact: true }).click();
    },
    JOIN: async (world) => {
      await world.page.getByLabel("Player name", { exact: true }).fill("Ada");
      await world.page.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(world.page.getByTestId("player-identity")).toBeVisible();
      world.playerId = (await world.page.getByTestId("player-identity").textContent())!;
    },
    RELOAD: async ({ page }) => {
      await page.reload();
    },
    BACK: async ({ page }) => {
      await page.goBack();
    },
    FORWARD: async ({ page }) => {
      await page.goForward();
    },
    RETURN_TO_BOARD: async ({ page }) => {
      await page.getByRole("link", { name: "Return to board", exact: true }).click();
    },
    START: async ({ page, context, invite, playerId }) => {
      const other = await context.newPage();
      await other.goto(invite);
      await other.getByLabel("Player name", { exact: true }).fill("Ada");
      await other.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(other.getByTestId("player-identity")).toBeVisible();
      expect(await other.getByTestId("player-identity").textContent()).not.toBe(playerId);
      await expect(roster(page)).toHaveCount(2);
      await page.getByRole("button", { name: "Start game", exact: true }).click();
      await expect(
        other.getByRole("region", { name: "Your tiles", exact: true }).getByRole("listitem"),
      ).toHaveCount(7);
    },
    RESUME_PLAYER: async ({ page, invite }) => {
      await page.goto(invite);
    },
  },
  states: {
    board: async ({ page }) => {
      await expect(page.getByRole("link", { name: "Join as player", exact: true })).toBeVisible();
    },
    entry: async ({ page }) => {
      await expect(page.getByLabel("Player name", { exact: true })).toHaveValue("");
      expect(page.context().pages()).toHaveLength(1);
    },
    player: async ({ page, playerId, boardId }) => {
      expect(playerId).not.toBe(`Player ${boardId}`);
      await expect(page.getByTestId("player-identity")).toHaveText(playerId);
    },
    refreshedPlayer: async ({ page, playerId }) => {
      await expect(page.getByTestId("player-identity")).toHaveText(playerId);
    },
    recoveredBoard: async ({ page, boardUrl }) => {
      await expect(roster(page)).toHaveCount(1);
      expect(page.url()).toBe(boardUrl);
    },
    resumedPlayer: async ({ page, playerId }) => {
      await expect(page.getByTestId("player-identity")).toHaveText(playerId);
    },
    hostBoard: async ({ page }) => {
      await expect(roster(page)).toHaveCount(1);
    },
    playingBoard: async ({ page }) => {
      await expect(page.getByRole("link", { name: "Join as player", exact: true })).toHaveCount(0);
      await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
      await expect(page.getByRole("region", { name: "Your tiles", exact: true })).toHaveCount(0);
    },
    playingPlayer: async ({ page, context, playerId }) => {
      await expect(page.getByTestId("player-identity")).toHaveText(playerId);
      const rack = page
        .getByRole("region", { name: "Your tiles", exact: true })
        .getByRole("listitem");
      await expect(rack).toHaveCount(7);
      const tiles = await rack.evaluateAll((items) =>
        items.map((item) => item.getAttribute("data-tile-id")),
      );
      await page.reload();
      await expect(page.getByTestId("player-identity")).toHaveText(playerId);
      await expect(rack).toHaveCount(7);
      expect(
        await rack.evaluateAll((items) => items.map((item) => item.getAttribute("data-tile-id"))),
      ).toEqual(tiles);
      const others = await context
        .pages()[0]!
        .getByRole("region", { name: "Your tiles", exact: true })
        .getByRole("listitem")
        .evaluateAll((items) => items.map((item) => item.getAttribute("data-tile-id")));
      expect(tiles.some((tile) => others.includes(tile))).toBe(false);
      const storage = await page.evaluate(() => Object.keys(sessionStorage));
      expect(storage.filter((key) => key.startsWith("scrabble:board:v1:"))).toHaveLength(1);
      expect(storage.filter((key) => key.startsWith("scrabble:player:v1:"))).toHaveLength(1);
      expect(new URL(page.url()).searchParams.has("token")).toBe(false);
    },
  },
});

test("public board link joins on touch without borrowing board ownership or a named seat", async ({
  page,
  browser,
}) => {
  const table = await openTable(page, browser, false);
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
  });
  try {
    await table.create();
    await table.join(table.ada, "Ada");
    const guest = await context.newPage();
    await guest.goto(`${page.url()}&name=Ada&playerId=2&token=${"a".repeat(32)}`);
    await guest.getByRole("link", { name: "Join as player", exact: true }).tap();
    await expect(guest.getByLabel("Player name", { exact: true })).toHaveValue("");
    expect(new URL(guest.url()).searchParams.has("token")).toBe(false);
    await guest.getByLabel("Player name", { exact: true }).fill("Ada");
    await guest.getByRole("button", { name: "Join game", exact: true }).tap();
    await expect(guest.getByTestId("player-identity")).toBeVisible();
    expect(await guest.getByTestId("player-identity").textContent()).not.toBe(
      await table.ada.getByTestId("player-identity").textContent(),
    );
    await expect(guest.getByRole("link", { name: "Return to board", exact: true })).toHaveCount(0);
    await expect(roster(page)).toHaveCount(2);
    expect(context.pages()).toHaveLength(1);
    expect(await guest.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    await expect(guest.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
  } finally {
    await context.close();
    await table.close();
  }
});

for (const [status, reason] of [
  [429, "game is full"],
  [409, "already started"],
  [410, "game has expired"],
  [404, "no longer available"],
  [503, "Unable to check"],
] as const) {
  test(`same-tab entry reports unavailable admission (${status}) and retries`, async ({ page }) => {
    await page.goto(`./?${fixtureQuery()}`);
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const invite = await readInvite(page);
    const url = new URL(invite);
    url.searchParams.delete("view");
    await page.route("**/games/*/join", (route) =>
      route.fulfill({
        status,
        contentType: "application/json",
        body: JSON.stringify({ error: "unavailable" }),
      }),
    );
    await page.goto(url.href);
    await expect(page.getByRole("alert").filter({ hasText: reason })).toBeVisible();
    await expect(page.getByRole("link", { name: "Join as player", exact: true })).toHaveCount(0);
    await page.unroute("**/games/*/join");
    await page.getByRole("button", { name: "Retry join", exact: true }).click();
    await expect(page.getByRole("link", { name: "Join as player", exact: true })).toBeVisible();
  });
}

test("same-tab entry handles a transport failure before offering retry", async ({ page }) => {
  await page.goto(`./?${fixtureQuery()}`);
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await readInvite(page);
  await page.route("**/games/*/join", (route) => route.abort("failed"));
  await page.reload();
  await expect(page.getByRole("alert").filter({ hasText: "Unable to check" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Join as player", exact: true })).toHaveCount(0);
  await page.unroute("**/games/*/join");
  await page.getByRole("button", { name: "Retry join", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join as player", exact: true })).toBeVisible();
});
