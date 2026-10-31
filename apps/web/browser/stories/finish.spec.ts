import { expect } from "../fixtures";
import { openTable, readInvite, roster, type Table } from "./match-table";
import { storyTests } from "./story-test";

type World = Table & { finishedMatch: string };

const follow = async (phone: Table["ada"]) => {
  await phone.getByRole("button", { name: "Join rematch", exact: true }).click();
};
const matchOf = (url: string) => new URL(url).searchParams.get("match");

storyTests<World>("FINISH-1", {
  open: async ({ page, browser }) => {
    const table = await openTable(page, browser, false);
    await table.seatAndStart();
    return Object.assign(table, { finishedMatch: matchOf(table.invite) ?? "" });
  },
  close: async (table) => void (await table.close()),
  initial: async (table) => {
    await expect.poll(() => table.boardState()?.public.phase.kind).toBe("playing");
  },
  events: {
    // game-completion.md: six scoreless turns end the two-player match.
    PASS_OUT: async (table) => {
      for (let turn = 0; turn < 6; turn++) {
        await expect
          .poll(() => {
            const phase = table.boardState()?.public.phase;
            return phase?.kind === "playing" ? phase.turn : undefined;
          })
          .toBe(String(turn));
        const phase = table.boardState()?.public.phase;
        await table.pass(
          phase?.kind === "playing" && phase.active_player === "2" ? table.ada : table.lin,
        );
      }
    },
    PHONE_RELOADS: async (table) => {
      await table.ada.reload();
    },
    BOARD_RELOADS: async (table) => {
      await table.board.reload();
    },
    START_REMATCH: (table) =>
      table.board.getByRole("button", { name: "Start rematch", exact: true }).click(),
    ADA_FOLLOWS: (table) => follow(table.ada),
    LIN_FOLLOWS: (table) => follow(table.lin),
    START_NEXT: (table) =>
      table.board.getByRole("button", { name: "Start game", exact: true }).click(),
  },
  states: {
    playing: async () => {},
    finished: async (table) => {
      await expect.poll(() => table.boardState()?.public.phase.kind).toBe("finished");
      await expect(table.board.getByText("Game finished", { exact: true })).toBeVisible();
      await expect(
        table.board
          .getByRole("heading", { name: "Final standings", exact: true })
          .or(table.board.getByRole("list", { name: "Final standings", exact: true }))
          .first(),
      ).toBeVisible();
      await expect(
        table.board.getByRole("button", { name: "Start rematch", exact: true }),
      ).toBeVisible();
      for (const phone of [table.ada, table.lin]) {
        await expect(phone.getByText("Game finished.", { exact: true })).toBeVisible();
        await expect(
          phone.getByText(/Unplayed tiles: −\d+ points|You used all your tiles\./),
        ).toBeVisible();
        for (const name of ["Commit move", "Pass turn", "Exchange tiles"]) {
          await expect(phone.getByRole("button", { name, exact: true })).toHaveCount(0);
        }
      }
    },
    finishedElsewhere: async (table) => {
      await expect(table.board.getByText("Game finished", { exact: true })).toBeVisible();
      await expect(
        table.board.getByText("Start a rematch from the board that created this game."),
      ).toBeVisible();
      await expect(
        table.board.getByRole("button", { name: "Start rematch", exact: true }),
      ).toHaveCount(0);
    },
    rematchLobby: async (table) => {
      const invite = await readInvite(table.board);
      expect(matchOf(invite)).toMatch(/^b_/);
      expect(matchOf(invite)).not.toBe(table.finishedMatch);
      expect(matchOf(table.board.url())).toBe(matchOf(invite));
      await expect(roster(table.board)).toHaveCount(0);
      for (const phone of [table.ada, table.lin]) {
        await expect(
          phone.getByRole("button", { name: "Join rematch", exact: true }),
        ).toBeVisible();
      }
    },
    adaFollowed: async (table) => {
      await expect(roster(table.board)).toHaveCount(1);
      await expect(roster(table.board).first()).toContainText("Ada");
      expect(matchOf(table.ada.url())).toBe(matchOf(table.board.url()));
      expect(new URL(table.ada.url()).searchParams.get("name")).toBeNull();
      // A player who never taps is not seated and keeps the finished view.
      await expect(table.lin.getByText("Game finished.", { exact: true })).toBeVisible();
      expect(matchOf(table.lin.url())).toBe(table.finishedMatch);
    },
    bothFollowed: async (table) => {
      await expect(roster(table.board)).toHaveCount(2);
      await expect(
        table.board.getByRole("button", { name: "Start game", exact: true }),
      ).toBeEnabled();
    },
    rematchPlaying: async (table) => {
      await expect(
        table.board.getByRole("link", { name: "Join this game", exact: true }),
      ).toHaveCount(0);
      await expect(
        table.board.getByRole("button", { name: "Start game", exact: true }),
      ).toHaveCount(0);
      await expect(
        table.ada.getByRole("list", { name: "Your rack", exact: true }).getByRole("listitem"),
      ).toHaveCount(7);
      await expect(
        table.lin.getByRole("list", { name: "Your rack", exact: true }).getByRole("listitem"),
      ).toHaveCount(7);
    },
  },
});
