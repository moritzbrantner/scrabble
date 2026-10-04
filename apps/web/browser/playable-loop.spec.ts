import { expect, test, type Page } from "@playwright/test";
import { selectOpening } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

test("built Pages client plays consecutive turns through isolated phones", async ({
  page,
  browser,
}) => {
  const firstContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const secondContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const first = await firstContext.newPage();
  const second = await secondContext.newPage();
  const boardState = await observeSnapshots(page, "1", true);
  const firstState = await observeSnapshots(first, "2");
  const secondState = await observeSnapshots(second, "3");
  try {
    await page.goto("./");
    expect(await page.locator("script[src]").first().getAttribute("src")).toMatch(
      /^\/scrabble\/assets\/.*\.js$/,
    );
    await selectOpening(page);
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const href = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    expect(href).not.toBeNull();
    if (href === null) {
      throw new Error("Missing invite");
    }
    expect([...new URL(href).searchParams.keys()].sort()).toEqual(["match", "view"]);
    for (const [phone, name] of [
      [first, "Ada"],
      [second, "Lin"],
    ] as const) {
      await phone.goto(href);
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => firstState()?.own_rack.tiles.length).toBe(7);
    await expect.poll(() => secondState()?.own_rack.tiles.length).toBe(7);
    const rackIds = () => [
      firstState()?.own_rack.tiles.map((tile) => tile.id) ?? [],
      secondState()?.own_rack.tiles.map((tile) => tile.id) ?? [],
    ];
    const initial = rackIds();
    expect(new Set(initial.flat()).size).toBe(14);
    for (const [phone, recipient] of [
      [first, "2"],
      [second, "3"],
    ] as const) {
      const stored = await phone.evaluate(() =>
        Object.keys(sessionStorage).map((key) => {
          const value: unknown = JSON.parse(sessionStorage.getItem(key) ?? "null");
          return value !== null && typeof value === "object" && "playerId" in value
            ? value.playerId
            : null;
        }),
      );
      expect(stored).toEqual([recipient]);
    }
    const commit = (phone: Page) => phone.getByRole("button", { name: "Commit move", exact: true });
    const waiting = async (phone: Page) => {
      const rack = phone.getByRole("list", { name: "Your rack", exact: true });
      await expect(rack).toHaveAttribute("aria-disabled", "true");
      await expect(rack.locator("button:disabled")).toHaveCount(7);
      await expect(commit(phone)).toBeDisabled();
      await expect(phone.getByRole("button", { name: "Pass turn", exact: true })).toBeDisabled();
      await expect(
        phone.getByRole("button", { name: "Exchange tiles", exact: true }),
      ).toBeDisabled();
    };
    await expect(commit(second)).toBeDisabled();
    await waiting(second);
    const place = async (phone: Page, letter: string, column: number) => {
      await phone
        .getByRole("list", { name: "Your rack", exact: true })
        .getByRole("button", { name: new RegExp(`^${letter},`) })
        .first()
        .click();
      await phone
        .getByRole("region", { name: "Move editor", exact: true })
        .getByRole("button", { name: new RegExp(`^Row 8, column ${column}:`) })
        .click();
    };
    const converge = async (turn: string, word: string, scores: number[], remaining: number) => {
      await expect
        .poll(() => {
          const board = boardState()?.public;
          return (
            board?.phase.kind === "playing" &&
            board.phase.turn === turn &&
            JSON.stringify(firstState()?.public) === JSON.stringify(board) &&
            JSON.stringify(secondState()?.public) === JSON.stringify(board)
          );
        })
        .toBe(true);
      const board = boardState();
      expect(board?.own_rack.tiles.length).toBe(0);
      expect(board?.public.board.map((tile) => tile.letter).join("")).toBe(word);
      expect(board?.public.players.map((player) => player.score)).toEqual(scores);
      expect(board?.public.remaining_tiles).toBe(remaining);
      expect(board?.public.preview).toBeNull();
      for (const phone of [first, second]) {
        await expect(
          phone
            .getByRole("region", { name: "Move editor", exact: true })
            .locator(".committed-square"),
        ).toHaveCount(word.length);
        await expect(
          phone.getByRole("list", { name: "Your rack", exact: true }).getByRole("listitem"),
        ).toHaveCount(7);
        expect(
          await phone
            .getByRole("list", { name: "Players and scores", exact: true })
            .locator("strong")
            .allTextContents(),
        ).toEqual(scores.map(String));
      }
      expect(new Set(rackIds().flat()).size).toBe(14);
    };
    expect(boardState()?.public.dictionary.name).toBe("scrabble-browser-custom");
    expect(boardState()?.public.dictionary.revision).toMatch(/^1\+sha256:[0-9a-f]{64}$/);
    expect(boardState()?.public.ruleset.revision).toMatch(/^1\+dictionary-sha256:[0-9a-f]{64}$/);
    await place(first, "A", 8);
    await place(first, "T", 9);
    await commit(first).click();
    const openingEditor = first.getByRole("region", { name: "Move editor", exact: true });
    await expect(openingEditor.getByRole("status")).toContainText("Not in the dictionary: AT.");
    await expect(page.locator(".letter-tile")).toHaveCount(0);
    expect(rackIds()).toEqual(initial);
    await openingEditor.getByRole("button", { name: "Cancel move", exact: true }).click();
    await place(first, "T", 8);
    await place(first, "A", 9);
    await expect(page.locator(".tentative-tile")).toHaveCount(2);
    await commit(first).click();
    await converge("1", "TA", [4, 0], 84);
    const afterFirst = rackIds();
    expect(afterFirst[0]?.filter((id) => !initial[0]?.includes(id))).toHaveLength(2);
    expect(afterFirst[1]).toEqual(initial[1]);
    await expect(first.getByText("Waiting for Lin.", { exact: true })).toBeVisible();
    await expect(commit(first)).toBeDisabled();
    await waiting(first);
    await expect(second.getByText("It is your turn.", { exact: true })).toBeVisible();
    await place(second, "C", 7);
    await expect(page.locator(".tentative-tile")).toHaveCount(1);
    await expect(page.getByRole("cell", { name: /^Row 8, column 7: Tentative C/ })).toBeVisible();
    await commit(second).click();
    await converge("2", "CTA", [4, 5], 83);
    const afterSecond = rackIds();
    expect(afterSecond[0]).toEqual(afterFirst[0]);
    expect(afterSecond[1]?.filter((id) => !afterFirst[1]?.includes(id))).toHaveLength(1);
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();
    await expect(second.getByText("Waiting for Ada.", { exact: true })).toBeVisible();
    await expect(commit(second)).toBeDisabled();
    await waiting(second);
    await page.reload();
    await expect(page.getByText("Turn 3", { exact: true })).toBeVisible();
    await expect(page.locator(".letter-tile")).toHaveCount(3);
    await expect.poll(() => boardState()?.own_rack.tiles.length).toBe(0);
    const nextTile = first
      .getByRole("list", { name: "Your rack", exact: true })
      .getByRole("button")
      .first();
    const blank = (await nextTile.getAttribute("aria-label"))?.startsWith("Blank tile");
    await nextTile.click();
    const editor = first.getByRole("region", { name: "Move editor", exact: true });
    await editor.getByRole("button", { name: /^Row 7, column 8:/ }).click();
    if (blank) {
      await editor
        .getByRole("group", { name: "Choose blank letter", exact: true })
        .getByRole("button", { name: "A", exact: true })
        .click();
    }
    await expect(page.locator(".tentative-tile")).toHaveCount(1);
    const pass = first.getByRole("button", { name: "Pass turn", exact: true });
    await pass.click();
    const confirmation = first.getByRole("group", { name: "Confirm pass", exact: true });
    await expect(confirmation).toBeVisible();
    expect(boardState()?.public.phase).toMatchObject({ kind: "playing", turn: "2" });
    await confirmation.getByRole("button", { name: "Keep playing", exact: true }).click();
    await expect(confirmation).toHaveCount(0);
    await expect(pass).toBeFocused();
    await expect(editor.locator(".tentative-square")).toHaveCount(1);
    await pass.click();
    await confirmation.getByRole("button", { name: "Confirm pass", exact: true }).click();
    await converge("3", "CTA", [4, 5], 83);
    expect(rackIds()).toEqual(afterSecond);
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
    await expect(editor.locator(".tentative-square")).toHaveCount(0);
    await waiting(first);
    await expect(second.getByText("It is your turn.", { exact: true })).toBeVisible();
    const secondEditor = second.getByRole("region", { name: "Move editor", exact: true });
    const secondRack = second.getByRole("list", { name: "Your rack", exact: true });
    const selectedIds = afterSecond[1]?.slice(0, 2) ?? [];
    expect(selectedIds).toHaveLength(2);
    const draftTile = secondRack.getByRole("button").first();
    const draftBlank = (await draftTile.getAttribute("aria-label"))?.startsWith("Blank tile");
    await draftTile.click();
    await secondEditor.getByRole("button", { name: /^Row 7, column 8:/ }).click();
    if (draftBlank) {
      await secondEditor
        .getByRole("group", { name: "Choose blank letter", exact: true })
        .getByRole("button", { name: "A", exact: true })
        .click();
    }
    await expect(page.locator(".tentative-tile")).toHaveCount(1);
    const exchange = second.getByRole("button", { name: "Exchange tiles", exact: true });
    await exchange.click();
    const exchangeGroup = second.getByRole("group", { name: "Exchange tiles", exact: true });
    const confirmExchange = exchangeGroup.getByRole("button", {
      name: "Confirm exchange",
      exact: true,
    });
    await expect(confirmExchange).toBeDisabled();
    await expect(secondRack.getByRole("button").first()).toBeFocused();
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
    for (const id of selectedIds) {
      const tile = secondRack.locator(`[data-tile-id="${id}"]`).getByRole("button");
      await tile.click();
      await expect(tile).toHaveAttribute("aria-pressed", "true");
    }
    const toggleTile = secondRack.getByRole("button").first();
    await toggleTile.click();
    await expect(toggleTile).toHaveAttribute("aria-pressed", "false");
    await toggleTile.click();
    expect(rackIds()).toEqual(afterSecond);
    expect(boardState()?.public.phase).toMatchObject({ kind: "playing", turn: "3" });
    await exchangeGroup.getByRole("button", { name: "Keep playing", exact: true }).click();
    await expect(exchange).toBeFocused();
    await expect(secondEditor.locator(".tentative-square")).toHaveCount(1);
    await expect(page.locator(".tentative-tile")).toHaveCount(1);
    await exchange.click();
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
    for (const id of selectedIds) {
      await secondRack.locator(`[data-tile-id="${id}"]`).getByRole("button").click();
    }
    await confirmExchange.focus();
    await confirmExchange.press("Enter");
    await converge("4", "CTA", [4, 5], 83);
    const afterExchange = rackIds();
    expect(afterExchange[0]).toEqual(afterSecond[0]);
    expect(afterExchange[1]?.slice(0, 5)).toEqual(afterSecond[1]?.slice(2));
    expect(afterExchange[1]?.filter((id) => !afterSecond[1]?.includes(id))).toHaveLength(2);
    expect(afterExchange[1]?.some((id) => selectedIds.includes(id))).toBe(false);
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
    await expect(secondEditor.locator(".tentative-square")).toHaveCount(0);
    await waiting(second);
    await second.reload();
    await expect
      .poll(() => secondState()?.own_rack.tiles.map((tile) => tile.id))
      .toEqual(afterExchange[1]);
    await expect(second.getByText("Waiting for Ada.", { exact: true })).toBeVisible();
  } finally {
    await firstContext.close();
    await secondContext.close();
  }
});
