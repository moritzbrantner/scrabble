import { expect, type Locator } from "../fixtures";
import { openTable, type Table } from "./match-table";
import { storyTests } from "./story-test";

type World = Table & { rack: readonly string[] };

const editor = (table: Table) =>
  table.ada.getByRole("region", { name: "Move editor", exact: true });
const button = (scope: Locator | Table["ada"], name: string) =>
  scope.getByRole("button", { name, exact: true });
const rackIds = (table: Table) => table.adaState()?.own_rack.tiles.map((tile) => tile.id) ?? [];

async function place(table: Table, letters: readonly string[]) {
  const rack = table.ada.getByRole("list", { name: "Your rack", exact: true });
  for (const [index, letter] of letters.entries()) {
    await rack
      .getByRole("button", { name: new RegExp(`^${letter},`) })
      .first()
      .click();
    await editor(table)
      .getByRole("button", { name: new RegExp(`^Row 8, column ${8 + index}:`) })
      .click();
  }
}

/** Decision 2.6: drafts stay on the drafting phone. */
async function draftsArePrivate(table: Table) {
  await expect(table.board.locator(".tentative-tile, .tentative-square")).toHaveCount(0);
  await expect(table.lin.locator(".tentative-tile, .tentative-square")).toHaveCount(0);
  expect(table.linState()?.own_preview).toBeUndefined();
  expect(table.boardState()?.public.board).toEqual([]);
}

/** The turn belongs to Lin; Ada's phone cannot act (Decision 2.8). */
async function linsTurn(table: Table, turn: string, boardTiles: number) {
  await expect
    .poll(() => {
      const phase = table.boardState()?.public.phase;
      return phase?.kind === "playing" ? [phase.active_player, phase.turn] : undefined;
    })
    .toEqual(["3", turn]);
  await expect.poll(() => table.boardState()?.public.board.length).toBe(boardTiles);
  await expect.poll(() => table.adaState()?.public.phase).toEqual(table.boardState()?.public.phase);
  await expect(table.lin.getByText("It is your turn.", { exact: true })).toBeVisible();
  await expect(table.ada.getByText("Waiting for Lin.", { exact: true })).toBeVisible();
  for (const name of ["Commit move", "Pass turn", "Exchange tiles"]) {
    await expect(button(table.ada, name)).toBeDisabled();
  }
  await expect(button(table.lin, "Pass turn")).toBeEnabled();
}

storyTests<World>("TURN-1", {
  open: async ({ page, browser }) => {
    const table = await openTable(page, browser, true);
    await table.seatAndStart();
    return Object.assign(table, { rack: rackIds(table) });
  },
  close: async (table) => void (await table.close()),
  initial: async (table) => {
    await expect(table.ada.getByText("It is your turn.", { exact: true })).toBeVisible();
  },
  events: {
    PLACE_AT: (table) => place(table, ["A", "T"]),
    PLACE_TA: (table) => place(table, ["T", "A"]),
    // The fixture dictionary accepts AT but not TA.
    COMMIT_REJECTED: (table) => button(editor(table), "Commit move").click(),
    CANCEL_DRAFT: (table) => button(editor(table), "Cancel move").click(),
    COMMIT: (table) => button(editor(table), "Commit move").click(),
    OPEN_PASS: (table) => button(table.ada, "Pass turn").click(),
    KEEP_PLAYING: (table) =>
      button(
        table.ada.getByRole("group", { name: "Confirm pass", exact: true }),
        "Keep playing",
      ).click(),
    CONFIRM_PASS: (table) => button(table.ada, "Confirm pass").click(),
    OPEN_EXCHANGE: (table) => button(table.ada, "Exchange tiles").click(),
    KEEP_PLAYING_EXCHANGE: (table) =>
      button(
        table.ada.getByRole("group", { name: "Exchange tiles", exact: true }),
        "Keep playing",
      ).click(),
    CONFIRM_EXCHANGE: async (table) => {
      const rack = table.ada.getByRole("list", { name: "Your rack", exact: true });
      for (const id of table.rack.slice(0, 2)) {
        await rack.locator(`[data-tile-id="${id}"]`).getByRole("button").click();
      }
      await button(
        table.ada.getByRole("group", { name: "Exchange tiles", exact: true }),
        "Confirm exchange",
      ).click();
    },
  },
  states: {
    adaTurn: async (table) => {
      await expect(editor(table).locator(".tentative-square")).toHaveCount(0);
      await expect(button(editor(table), "Commit move")).toBeDisabled();
      await expect(button(table.ada, "Pass turn")).toBeEnabled();
      await draftsArePrivate(table);
    },
    draftAt: async (table) => {
      await expect(editor(table).locator(".tentative-square")).toHaveCount(2);
      await expect.poll(() => table.adaState()?.own_preview?.tiles.length).toBe(2);
      await expect(button(editor(table), "Commit move")).toBeEnabled();
      await draftsArePrivate(table);
    },
    draftTa: async (table) => {
      await expect(editor(table).locator(".tentative-square")).toHaveCount(2);
      await draftsArePrivate(table);
    },
    rejected: async (table) => {
      await expect(editor(table).getByRole("status")).toContainText("Not in the dictionary: TA.");
      await expect(editor(table).locator(".tentative-square")).toHaveCount(2);
      expect(rackIds(table)).toEqual(table.rack);
      await draftsArePrivate(table);
    },
    passConfirm: async (table) => {
      await expect(
        table.ada.getByRole("group", { name: "Confirm pass", exact: true }),
      ).toBeVisible();
      expect(table.boardState()?.public.phase).toMatchObject({ turn: "0" });
    },
    exchanging: async (table) => {
      await expect(
        button(
          table.ada.getByRole("group", { name: "Exchange tiles", exact: true }),
          "Confirm exchange",
        ),
      ).toBeDisabled();
    },
    committed: async (table) => {
      await linsTurn(table, "1", 2);
      await expect
        .poll(() => rackIds(table).filter((id) => table.rack.includes(id)).length)
        .toBe(5);
      expect(rackIds(table)).toHaveLength(7);
      expect(table.boardState()?.public.players.map((player) => player.score)[0]).toBeGreaterThan(
        0,
      );
    },
    passed: async (table) => {
      await linsTurn(table, "1", 0);
      expect(rackIds(table)).toEqual(table.rack);
      await expect(editor(table).locator(".tentative-square")).toHaveCount(0);
    },
    exchanged: async (table) => {
      await linsTurn(table, "1", 0);
      await expect
        .poll(() => rackIds(table).filter((id) => table.rack.includes(id)).length)
        .toBe(5);
      expect(rackIds(table)).toHaveLength(7);
    },
  },
});
