// Acceptance for #33 (docs/specs/rematch.md): completed-game summary and one-tap rematch on the
// built Pages client against the real Rust server. Only public UI and HTTP are driven.
import { expect, test, type Page } from "@playwright/test";

import fixtures from "../src/fixtures/public.json" with { type: "json" };
import { gameFixture } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

const rackValue = (tiles: { face: { kind: string; letter?: string } }[]) =>
  tiles.reduce((sum, tile) => {
    const definition = fixtures.ruleset.tiles.find((entry) =>
      entry.face.kind === "blank"
        ? tile.face.kind === "blank"
        : tile.face.kind === "letter" && entry.face.letter === tile.face.letter,
    );
    if (definition === undefined) {
      throw new Error("Unknown rack face");
    }
    return sum + (tile.face.kind === "blank" ? 0 : definition.value);
  }, 0);

test("finished match shows a summary and the board starts a one-tap rematch", async ({
  page,
  browser,
  request,
}) => {
  test.setTimeout(120_000);
  const adaContext = await browser.newContext({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
    reducedMotion: "reduce",
  });
  const linContext = await browser.newContext({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
    reducedMotion: "reduce",
  });
  const ada = await adaContext.newPage();
  const lin = await linContext.newPage();
  // The tapping phone is not observed: its rematch admission order is not fixed by the spec.
  const boardState = await observeSnapshots(page, "1", true);
  const linState = await observeSnapshots(lin, "3");
  try {
    await page.goto("./");
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const invite = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    if (invite === null) {
      throw new Error("Missing invite");
    }
    const finishedId = new URL(invite).searchParams.get("match") ?? "";
    expect(finishedId).toMatch(/^b_/);
    const finishedBoardUrl = page.url();
    for (const [phone, name] of [
      [ada, "Ada"],
      [lin, "Lin"],
    ] as const) {
      await phone.goto(invite);
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => boardState()?.public.phase.kind).toBe("playing");

    // Finish the real match through six accepted passes (the scoreless-turn limit).
    const phones: Record<string, Page> = { "2": ada, "3": lin };
    for (let turn = 0; turn < 6; turn++) {
      await expect
        .poll(() => {
          const phase = boardState()?.public.phase;
          return phase?.kind === "playing" ? phase.turn : undefined;
        })
        .toBe(String(turn));
      const phase = boardState()?.public.phase;
      const phone = phase?.kind === "playing" ? phones[phase.active_player] : undefined;
      if (phone === undefined) {
        throw new Error("Active player is not a seated phone");
      }
      await phone.getByRole("button", { name: "Pass turn", exact: true }).click();
      await phone.getByRole("button", { name: "Confirm pass", exact: true }).click();
    }
    await expect.poll(() => boardState()?.public.phase.kind).toBe("finished");
    await expect.poll(() => linState()?.public.phase.kind).toBe("finished");
    const finished = boardState()?.public;
    if (finished === undefined) {
      throw new Error("Missing finished snapshot");
    }

    // Board summary: ranked final standings, highest first, ties in seat order.
    const ranked = finished.players
      .map((player, seat) => ({ player, seat }))
      .sort((left, right) => right.player.score - left.player.score || left.seat - right.seat)
      .map(({ player }) => player.display_name);
    await expect(page.getByText("Game finished", { exact: true })).toBeVisible();
    await expect(
      page
        .getByRole("heading", { name: "Final standings", exact: true })
        .or(page.getByRole("list", { name: "Final standings", exact: true }))
        .first(),
    ).toBeVisible();
    const standings = page
      .getByRole("complementary", { name: "Game status", exact: true })
      .getByRole("listitem");
    await expect(standings).toHaveCount(ranked.length);
    for (const [index, name] of ranked.entries()) {
      await expect(standings.nth(index)).toContainText(name);
    }
    await expect(
      page.getByRole("region", { name: "Turn history", exact: true }).getByRole("listitem"),
    ).toHaveCount(6);

    // Phone summary: its own unplayed-tile deduction under the match ruleset.
    const linRack = linState()?.own_rack.tiles ?? [];
    expect(linRack).toHaveLength(7);
    const deduction = rackValue(linRack);
    await expect(lin.getByText("Game finished.", { exact: true })).toBeVisible();
    await expect(
      lin.getByText(new RegExp(`^Unplayed tiles: [−-]${deduction} points?$`)),
    ).toBeVisible();
    await expect(lin.getByText("You used all your tiles.", { exact: true })).toHaveCount(0);

    // Before a rematch, the public link lookup reports none.
    const lookup = `${gameFixture.api}/games/${finishedId}/rematch`;
    const none = await request.get(lookup);
    expect(none.status()).toBe(404);
    expect(await none.json()).toEqual({ error: "no-rematch" });
    await expect(ada.getByRole("button", { name: "Join rematch", exact: true })).toHaveCount(0);

    // The creating board starts the rematch and shows the new lobby invite.
    const finishedHistory = linState()?.public.history;
    await page.getByRole("button", { name: "Start rematch", exact: true }).click();
    const rematchInvite = page.getByRole("link", { name: "Join this game", exact: true });
    await expect(rematchInvite).toBeVisible();
    const rematchHref = (await rematchInvite.getAttribute("href")) ?? "";
    const rematchUrl = new URL(rematchHref);
    const rematchId = rematchUrl.searchParams.get("match") ?? "";
    expect(rematchId).toMatch(/^b_/);
    expect(rematchId).not.toBe(finishedId);
    expect([...rematchUrl.searchParams.keys()].sort()).toEqual(["match", "view"]);
    expect(new URL(page.url()).searchParams.get("match")).toBe(rematchId);
    expect(page.url()).not.toMatch(/requestId|boardRequestId|token|Ada|Lin/i);
    await expect(page.getByRole("button", { name: "Start game", exact: true })).toBeVisible();
    const linked = await request.get(lookup);
    expect(linked.status()).toBe(200);
    expect((await linked.json()).matchId).toBe(rematchId);

    // Both finished phones discover the rematch; only Ada taps it.
    await expect(lin.getByRole("button", { name: "Join rematch", exact: true })).toBeVisible({
      timeout: 10_000,
    });
    const join = ada.getByRole("button", { name: "Join rematch", exact: true });
    await expect(join).toBeVisible({ timeout: 10_000 });
    await join.click();
    await expect(ada.getByRole("heading", { name: "Ada", exact: true })).toBeVisible();
    await expect(ada.getByLabel("Player name", { exact: true })).toHaveCount(0);
    await expect(ada.getByText("Waiting for the host to start.", { exact: true })).toBeVisible();
    const adaUrl = new URL(ada.url());
    expect(adaUrl.searchParams.get("view")).toBe("player");
    expect(adaUrl.searchParams.get("match")).toBe(rematchId);
    expect([...adaUrl.searchParams.keys()].sort()).toEqual(["match", "view"]);
    expect(ada.url()).not.toMatch(/token|reconnect|sequence|Ada/i);
    const routes = await ada.evaluate(() =>
      Object.keys(sessionStorage)
        .filter((key) => key.startsWith("scrabble:player:v1:"))
        .map((key) => {
          const value: unknown = JSON.parse(sessionStorage.getItem(key) ?? "null");
          return value !== null && typeof value === "object" && "route" in value
            ? String(value.route)
            : "";
        }),
    );
    const rematchRoute = routes.find((route) => route.includes(rematchId));
    expect(rematchRoute).toBeDefined();
    expect(routes.some((route) => route.includes(finishedId))).toBe(true);
    expect(rematchRoute).not.toContain(finishedId);

    const roster = page.getByRole("list", { name: "Players and scores", exact: true });
    await expect(roster.getByRole("listitem").filter({ hasText: "Ada" })).toHaveCount(1);
    await expect(roster.getByRole("listitem").filter({ hasText: "Lin" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Start game", exact: true })).toBeVisible();

    // The non-tapping phone keeps its finished, read-only view of the unchanged match.
    await expect(lin.getByText("Game finished.", { exact: true })).toBeVisible();
    for (const name of ["Commit move", "Pass turn", "Exchange tiles"]) {
      await expect(lin.getByRole("button", { name, exact: true })).toHaveCount(0);
    }
    expect(linState()?.public.phase).toEqual(finished.phase);
    expect(linState()?.public.history).toEqual(finishedHistory);
    expect(linState()?.public.players.map((player) => player.score)).toEqual(
      finished.players.map((player) => player.score),
    );
    const closed = await request.get(`${gameFixture.api}/games/${finishedId}/join`);
    expect(closed.status()).toBe(409);
    expect(await closed.json()).toEqual({ error: "join-closed" });

    // A reloaded finished board no longer holds the creation nonce.
    await page.goto(finishedBoardUrl);
    await expect(page.getByText("Game finished", { exact: true })).toBeVisible();
    await expect(
      page.getByText("Start a rematch from the board that created this game.", { exact: true }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Start rematch", exact: true })).toHaveCount(0);
  } finally {
    await adaContext.close();
    await linContext.close();
  }
});
