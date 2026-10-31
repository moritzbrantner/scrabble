import { expect, type Page } from "../fixtures";
import { openTable, roster, type Table } from "./match-table";
import { storyTests } from "./story-test";

const start = (table: Table) =>
  table.board.getByRole("button", { name: "Start game", exact: true });

async function identity(phone: Page) {
  return phone.getByTestId("player-identity").textContent();
}

type World = Table & { adaIdentity: string | null };

storyTests<World>("LOBBY-1", {
  open: async ({ page, browser }) =>
    Object.assign(await openTable(page, browser, false), { adaIdentity: null }),
  close: async (table) => void (await table.close()),
  initial: async () => {},
  events: {
    CREATE: (table) => table.create(),
    BOARD_RELOAD: (table) => table.board.reload().then(() => undefined),
    ADA_JOINS: async (table) => {
      await table.join(table.ada, "Ada");
      table.adaIdentity = await identity(table.ada);
    },
    ADA_RELOADS: async (table) => {
      await table.ada.reload();
      await expect(table.ada.getByRole("heading", { name: "Ada", exact: true })).toBeVisible();
      expect(await identity(table.ada)).toBe(table.adaIdentity);
    },
    BAD_NAME_REJECTED: async (table) => {
      await table.ada.goto(table.invite);
      await table.ada.getByLabel("Player name", { exact: true }).fill("   ");
      await table.ada.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(table.ada.getByRole("alert")).toContainText("1 to 32 characters");
    },
    LIN_JOINS: (table) => table.join(table.lin, "Lin"),
    // phone-joining.md: duplicate names are allowed and keep distinct identities.
    SECOND_ADA_JOINS: async (table) => {
      await table.join(table.lin, "Ada");
      expect(await identity(table.lin)).not.toBe(table.adaIdentity);
    },
    START: (table) => table.start(),
    LATE_JOIN_REFUSED: async (table) => {
      const late = await table.board.context().browser()!.newContext();
      try {
        const phone = await late.newPage();
        await phone.goto(table.invite);
        await phone.getByLabel("Player name", { exact: true }).fill("Late");
        await phone.getByRole("button", { name: "Join game", exact: true }).click();
        await expect(phone.getByRole("alert").first()).toContainText("already started");
      } finally {
        await late.close();
      }
    },
  },
  states: {
    noGame: async () => {},
    empty: async (table) => {
      await expect(roster(table.board)).toHaveCount(0);
      await expect(start(table)).toBeDisabled();
      await expect(
        table.board.getByRole("link", { name: "Join this game", exact: true }),
      ).toBeVisible();
    },
    oneSeated: async (table) => {
      await expect(roster(table.board)).toHaveCount(1);
      await expect(start(table)).toBeDisabled();
    },
    twoSeated: async (table) => {
      await expect(roster(table.board)).toHaveCount(2);
      await expect(start(table)).toBeEnabled();
    },
    started: async (table) => {
      await expect.poll(() => table.boardState()?.public.phase.kind).toBe("playing");
      // player-invites.md: the invite disappears once play starts.
      await expect(
        table.board.getByRole("link", { name: "Join this game", exact: true }),
      ).toHaveCount(0);
      expect(table.boardState()?.own_rack.tiles).toEqual([]);
      const racks = [table.adaState(), table.linState()].flatMap((view) =>
        view?.own_rack.tiles.map((tile) => tile.id),
      );
      expect(new Set(racks).size).toBe(14);
    },
  },
});
