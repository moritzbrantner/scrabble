// The board and two phones of one fresh match, driven only through real UI and observed on the
// real wire. Shared by the journey stories.
import { expect, type Browser, type Page } from "../fixtures";
import { fixtureQuery, freshOpening, gameFixture, selectOpening } from "../game-fixture";
import { observeSnapshots } from "../pages-observer";

export const boardUrl = () => `./?${fixtureQuery().toString()}`;

// Every match needs its own creation request: a reused one recovers the previous game.
let previousOpening = "";

export async function openTable(page: Page, browser: Browser, opening: boolean) {
  const contexts = await Promise.all([
    browser.newContext({ viewport: { width: 390, height: 844 } }),
    browser.newContext({ viewport: { width: 390, height: 844 } }),
  ]);
  const ada = await contexts[0]!.newPage();
  const lin = await contexts[1]!.newPage();
  const boardState = await observeSnapshots(page, "1", true);
  const adaState = await observeSnapshots(ada, "2");
  const linState = await observeSnapshots(lin, "3");
  const table = {
    board: page,
    ada,
    lin,
    boardState,
    adaState,
    linState,
    invite: "",
    close: () => Promise.all(contexts.map((context) => context.close())),
    async create() {
      await page.goto(boardUrl());
      if (opening) {
        const selected = freshOpening(previousOpening || gameFixture.opening.requestId);
        previousOpening = selected.requestId;
        await selectOpening(page, selected);
      }
      await page.getByRole("button", { name: "Create game", exact: true }).click();
      table.invite = await readInvite(page);
    },
    async join(phone: Page, name: string) {
      await phone.goto(table.invite);
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    },
    async start() {
      await page.getByRole("button", { name: "Start game", exact: true }).click();
      await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
      await expect.poll(() => linState()?.own_rack.tiles.length).toBe(7);
    },
    /** The two phones after a started match. */
    async seatAndStart() {
      await table.create();
      await table.join(ada, "Ada");
      await table.join(lin, "Lin");
      await table.start();
    },
    async pass(phone: Page) {
      await phone.getByRole("button", { name: "Pass turn", exact: true }).click();
      await phone.getByRole("button", { name: "Confirm pass", exact: true }).click();
    },
  };
  return table;
}
export type Table = Awaited<ReturnType<typeof openTable>>;

export async function readInvite(page: Page, label = "Join this game") {
  const link = page.getByRole("link", { name: label, exact: true });
  await expect(link).toBeVisible();
  return (await link.getAttribute("href")) ?? "";
}

export const roster = (page: Page) =>
  page.getByRole("list", { name: "Players and scores", exact: true }).getByRole("listitem");
