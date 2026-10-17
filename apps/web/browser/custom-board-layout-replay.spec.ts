// Independent #136 contract, authored on bf5f331 before runner/report changes.
// Browser observations are retained for SAME-match native verification only after
// graceful fixture shutdown. A browser pass alone does not prove native replay.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { z } from "zod";

import { gameFixture, selectOpening } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

const directory = dirname(process.env.SCRABBLE_GAME_FIXTURE!);
const query = new URLSearchParams({
  server: gameFixture.endpoint,
  api: gameFixture.api,
  certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
});
const opening = z.strictObject({ requestId: z.string(), requestedAt: z.number() });
const editor = (page: Page) =>
  page.getByRole("region", { name: "Board layout editor", exact: true });
const list = (page: Page) => page.getByRole("list", { name: "Saved board layouts", exact: true });
async function paintCenter(page: Page, premium: string) {
  await editor(page).getByRole("radio", { name: premium, exact: true }).check();
  await editor(page)
    .getByRole("grid", { name: "Layout cells", exact: true })
    .getByRole("button", { name: /^Row 4, column 4:/ })
    .click();
}
async function save(page: Page) {
  await editor(page).getByRole("button", { name: "Save layout", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Layout saved." })).toBeVisible();
}

for (const shape of ["square", "hex"] as const) {
  test(`a completed custom ${shape} match replays independently of its edited and deleted source`, async ({
    browser,
  }) => {
    test.setTimeout(120_000);
    const context = await browser.newContext();
    const board = await context.newPage();
    const ada = await context.newPage();
    const lin = await context.newPage();
    const boardState = await observeSnapshots(board, "1", true);
    const adaState = await observeSnapshots(ada, "2");
    const linState = await observeSnapshots(lin, "3");
    const name = `Replay ${shape}`;
    try {
      await board.goto(`./?${query}`);
      await board.getByRole("button", { name: "New custom board", exact: true }).click();
      await editor(board).getByRole("textbox", { name: "Layout name", exact: true }).fill(name);
      await editor(board)
        .getByRole("radio", { name: shape === "hex" ? "Hexagon" : "Square", exact: true })
        .check();
      await editor(board).getByRole("spinbutton", { name: "Board size", exact: true }).fill("7");
      await paintCenter(board, "Triple word");
      if (shape === "hex") {
        await editor(board)
          .getByRole("radio", { name: /main word only/i })
          .check();
      }
      await save(board);
      await board
        .getByRole("combobox", { name: "Word list", exact: true })
        .selectOption("acceptance-hex-words");
      await board
        .getByRole("combobox", { name: "Tile profile", exact: true })
        .selectOption("acceptance-multi-letter-tiles");
      const picker = board.getByRole("combobox", { name: "Board layout", exact: true });
      const option = picker.getByRole("option").filter({ hasText: name });
      await picker.selectOption((await option.getAttribute("value"))!);
      const deal = opening.parse(
        JSON.parse(
          execFileSync(
            resolve(
              process.env.CARGO_TARGET_DIR ?? "target",
              "debug/examples/browser_game_fixture",
            ),
            ["--hex-opening", directory],
            { encoding: "utf8", timeout: 60_000 },
          ),
        ),
      );
      await selectOpening(board, deal);
      await board.getByRole("button", { name: "Create game", exact: true }).click();
      const href = await board
        .getByRole("link", { name: "Join this game", exact: true })
        .getAttribute("href");
      expect(href).toBeTruthy();
      for (const [page, name] of [
        [ada, "Ada"],
        [lin, "Lin"],
      ] as const) {
        await page.goto(href!);
        await page.getByLabel("Player name", { exact: true }).fill(name);
        await page.getByRole("button", { name: "Join game", exact: true }).click();
      }
      await board.getByRole("button", { name: "Start game", exact: true }).click();
      await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
      const configuration = structuredClone(boardState()!.public.configuration);
      expect(configuration.board_shape ?? "square").toBe(shape === "hex" ? "hexagon" : "square");
      expect(configuration.board_size).toBe(7);
      expect(configuration.premiums[shape === "hex" ? 18 : 24]).toBe("triple_word");
      for (const [face, column] of [
        ["SCH", 4],
        ["A", 5],
      ] as const) {
        await ada
          .getByRole("list", { name: "Your rack", exact: true })
          .getByRole("button", { name: new RegExp(`^${face}, \\d+ points$`) })
          .first()
          .click();
        await ada
          .getByRole("region", { name: "Move editor", exact: true })
          .getByRole("button", { name: new RegExp(`^Row 4, column ${column}:`) })
          .click();
      }
      await ada.getByRole("button", { name: "Commit move", exact: true }).click();
      await expect.poll(() => boardState()?.public.history.length).toBe(1);
      expect(boardState()!.public.history[0]!.action).toMatchObject({
        kind: "commit",
        words: ["SCHA"],
        move_score: 27,
      });
      const library = await context.newPage();
      await library.goto(`./?${query}`);
      await list(library)
        .getByRole("button", { name: `Edit ${name}`, exact: true })
        .click();
      await paintCenter(library, "Normal square");
      await save(library);
      await list(library)
        .getByRole("button", { name: `Delete ${name}`, exact: true })
        .click();
      await expect(list(library).getByRole("listitem").filter({ hasText: name })).toHaveCount(0);
      await library.close();
      expect(boardState()!.public.configuration).toEqual(configuration);
      // Complete the SAME real match through authoritative scoreless-turn rules.
      for (let turn = 0; turn < 20 && boardState()!.public.phase.kind !== "finished"; turn += 1) {
        const phase = boardState()!.public.phase;
        expect(phase.kind).toBe("playing");
        if (phase.kind !== "playing") {
          throw new Error("Expected continuing match");
        }
        const player = phase.active_player === "2" ? ada : lin;
        const state = phase.active_player === "2" ? adaState : linState;
        await expect.poll(() => state()?.public.phase).toEqual(phase);
        const previous = boardState()!.public.history.length;
        await player.getByRole("button", { name: "Pass turn", exact: true }).click();
        await player.getByRole("button", { name: "Confirm pass", exact: true }).click();
        await expect.poll(() => boardState()?.public.history.length).toBe(previous + 1);
      }
      await expect.poll(() => boardState()?.public.phase.kind).toBe("finished");
      await expect.poll(() => adaState()?.public).toEqual(boardState()!.public);
      await expect.poll(() => linState()?.public).toEqual(boardState()!.public);
      expect(boardState()!.public.configuration).toEqual(configuration);
      const match = new URL(board.url()).searchParams.get("match");
      expect(match).toBeTruthy();
      const state = boardState()!.public;
      const expectations = join(directory, "custom-layout-replay-expectations");
      mkdirSync(expectations, { recursive: true, mode: 0o700 });
      writeFileSync(
        join(expectations, `custom-${shape}.json`),
        JSON.stringify({
          match,
          configuration,
          game: {
            game_id: state.game_id,
            phase: state.phase,
            history: state.history,
            // Connection presence is runtime evidence, explicitly excluded from canonical state.
            players: state.players.map(({ connected: _connected, ...result }) => result),
          },
        }),
        { mode: 0o600 },
      );
    } finally {
      await context.close();
    }
  });
}
