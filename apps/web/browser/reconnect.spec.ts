import { expect, test, type Page } from "@playwright/test";

import { gameFixture, selectOpening } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

/** Close a real QUIC connection below BrowserMatch without supplying application state. */
async function interruptible(page: Page) {
  await page.addInitScript(() => {
    const NativeTransport = WebTransport;
    let count = 0;
    window.WebTransport = class extends NativeTransport {
      constructor(url: string | URL, options?: WebTransportOptions) {
        super(url, options);
        count += 1;
        document.documentElement.dataset.connections = String(count);
        window.addEventListener("test-interrupt-transport", () => this.close(), { once: true });
      }
    };
  });
}

test("real board and phone resume interrupted connections without changing authority or draft", async ({
  page,
  browser,
}) => {
  test.setTimeout(45_000);
  const phones = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const first = await phones.newPage();
  const second = await phones.newPage();
  const boardState = await observeSnapshots(page, "1", true);
  const firstState = await observeSnapshots(first, "2", false, true);
  const secondState = await observeSnapshots(second, "3");
  for (const client of [page, first, second]) {
    await interruptible(client);
  }
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  try {
    await page.goto(
      `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`,
    );
    await selectOpening(page, gameFixture.reconnectOpening);
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const href = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    expect(href).not.toBeNull();
    for (const [phone, name] of [
      [first, "Ada"],
      [second, "Lin"],
    ] as const) {
      await phone.goto(href ?? "");
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => firstState()?.own_rack.tiles.length).toBe(7);
    await expect.poll(() => secondState()?.own_rack.tiles.length).toBe(7);
    const before = firstState();
    const other = secondState()?.own_rack;
    const gameId = boardState()?.public.game_id;
    const route = new URL(page.url());
    route.searchParams.set("lang", "en");
    const editor = first.getByRole("region", { name: "Move editor", exact: true });
    const rack = first.getByRole("list", { name: "Your rack", exact: true });
    for (const [index, letter] of ["A", "T"].entries()) {
      await rack
        .getByRole("button", { name: new RegExp(`^${letter},`) })
        .first()
        .click();
      await editor
        .getByRole("button", { name: new RegExp(`^Row 8, column ${8 + index}:`) })
        .click();
    }
    await expect(page.locator(".tentative-tile")).toHaveCount(2);
    for (const client of [page, first]) {
      await client.getByText("Settings", { exact: true }).click();
      await client.getByLabel("Interface language", { exact: true }).selectOption("de");
      await expect(client.locator("html")).toHaveAttribute("lang", "de");
      await client.getByLabel("Sprache der Oberfläche", { exact: true }).selectOption("en");
      await expect(client.locator("html")).toHaveAttribute("lang", "en");
      expect(await client.evaluate(() => document.documentElement.dataset.connections)).toBe("1");
    }
    expect(firstState()?.own_rack).toEqual(before?.own_rack);
    expect(firstState()?.public.phase).toEqual(before?.public.phase);
    await expect(editor.locator(".tentative-square")).toHaveCount(2);
    await first.evaluate(() => window.dispatchEvent(new Event("test-interrupt-transport")));
    await expect
      .poll(() => first.evaluate(() => document.documentElement.dataset.connections))
      .toBe("2");
    await expect(first.getByText("Restoring game…", { exact: true })).toBeVisible();
    await expect(editor.getByRole("button", { name: "Commit move", exact: true })).toBeDisabled();
    await expect(rack).toHaveAttribute("aria-disabled", "true");
    await first.evaluate(() => window.dispatchEvent(new Event("test-release-snapshot")));
    await expect(first.getByText("Connected · Player 2", { exact: true })).toBeVisible();
    expect(firstState()?.own_rack).toEqual(before?.own_rack);
    expect(firstState()?.public.phase).toEqual(before?.public.phase);
    expect(secondState()?.own_rack).toEqual(other);
    await expect(editor.locator(".tentative-square")).toHaveCount(2);
    await expect(page.locator(".tentative-tile")).toHaveCount(2);
    await editor.getByRole("button", { name: "Commit move", exact: true }).click();
    await expect.poll(() => boardState()?.public.board.length).toBe(2);
    await expect
      .poll(() => firstState()?.public.phase)
      .toEqual({ kind: "playing", active_player: "3", turn: "1" });
    const committed = boardState()?.public.board;
    const updatedRack = firstState()?.own_rack;
    await page.evaluate(() => window.dispatchEvent(new Event("test-interrupt-transport")));
    await expect
      .poll(() => page.evaluate(() => document.documentElement.dataset.connections))
      .toBe("2");
    await expect(page.getByText("Connected · Player 1", { exact: true })).toBeVisible();
    expect(boardState()?.public.game_id).toBe(gameId);
    expect(boardState()?.public.board).toEqual(committed);
    expect(boardState()?.own_rack.tiles).toEqual([]);
    expect(page.url()).toBe(route.href);
    await first.reload();
    await expect(first.getByText("Connected · Player 2", { exact: true })).toBeVisible();
    expect(firstState()?.own_rack).toEqual(updatedRack);
    expect(firstState()?.public.board).toEqual(committed);
    await page.reload();
    await expect(page.getByText("Connected · Player 1", { exact: true })).toBeVisible();
    expect(boardState()?.public.game_id).toBe(gameId);
    expect(boardState()?.public.board).toEqual(committed);
    expect(boardState()?.public.players.map((player) => player.id)).toEqual(["2", "3"]);
    // A syntactically valid but unknown credential exercises actual native admission refusal.
    await first.evaluate(() => {
      const key = Object.keys(sessionStorage).find((entry) =>
        entry.startsWith("scrabble:player:v1:"),
      );
      if (key === undefined) {
        throw new Error("Missing private session");
      }
      const saved: unknown = JSON.parse(sessionStorage.getItem(key) ?? "null");
      if (saved === null || typeof saved !== "object" || !("token" in saved)) {
        throw new Error("Missing token");
      }
      saved.token = "00".repeat(16);
      sessionStorage.setItem(key, JSON.stringify(saved));
    });
    await first.reload();
    await expect(
      first.getByRole("alert").filter({ hasText: "Reconnect attempts have stopped" }).first(),
    ).toBeVisible({ timeout: 15_000 });
    await expect(
      first.getByRole("button", { name: "Retry player connection", exact: true }),
    ).toBeVisible();
    const attempts = await first.evaluate(() => document.documentElement.dataset.connections);
    expect(attempts).toBe("4");
    await first.waitForTimeout(750);
    expect(await first.evaluate(() => document.documentElement.dataset.connections)).toBe(attempts);
    expect(boardState()?.public.board).toEqual(committed);
    expect(boardState()?.public.players.map((player) => player.id)).toEqual(["2", "3"]);
    await expect(first.getByRole("button", { name: "Join game", exact: true })).toHaveCount(0);
  } finally {
    await phones.close();
  }
});
