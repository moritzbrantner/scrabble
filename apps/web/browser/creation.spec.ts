import { expect, test, type Page } from "@playwright/test";
import { z } from "zod";
import jsQR from "jsqr";
import { type PublicSnapshot } from "../src/public-state";
import { gameFixture as fixture, selectOpening } from "./game-fixture";

type Observed = {
  tick: string;
  gameId: string;
  order: string[];
  active: string | null;
  turn: string | null;
  remaining: number;
  rackCount: number;
  tileIds: string[];
  recipient: string;
  board: PublicSnapshot["board"];
  scores: PublicSnapshot["players"];
};
declare global {
  // oxlint-disable-next-line typescript/consistent-type-definitions -- DOM Window requires declaration merging.
  interface Window {
    scrabbleObserved?: Observed;
  }
}
/** Observe the decoded wire seam without persisting payloads or exposing credentials. */
async function observeProjection(page: Page) {
  await page.evaluate(async () => {
    const wirePath = "/scrabble/src/transport/wire.ts";
    const { SnapshotReassembler } = (await import(
      wirePath
    )) as typeof import("../src/transport/wire");
    const gamePath = "/scrabble/src/game-protocol.ts";
    const { decodePlayerSnapshot } = (await import(
      gamePath
    )) as typeof import("../src/game-protocol");
    // oxlint-disable-next-line typescript/unbound-method -- The wrapper explicitly binds each reassembler below.
    const original = SnapshotReassembler.prototype.accept;
    SnapshotReassembler.prototype.accept = function (bytes: Uint8Array) {
      const frame = original.call(this, bytes);
      if (frame !== undefined) {
        const raw: unknown = JSON.parse(new TextDecoder().decode(frame.payload));
        // The application independently validates the recipient against its welcome.
        if (
          raw !== null &&
          typeof raw === "object" &&
          "own_rack" in raw &&
          raw.own_rack !== null &&
          typeof raw.own_rack === "object" &&
          "player_id" in raw.own_rack &&
          typeof raw.own_rack.player_id === "string"
        ) {
          const snapshot = decodePlayerSnapshot(frame.payload, raw.own_rack.player_id);
          window.scrabbleObserved = {
            tick: frame.tick.toString(),
            gameId: snapshot.public.game_id,
            order: snapshot.public.players.map((player) => player.id),
            active:
              snapshot.public.phase.kind === "playing" ? snapshot.public.phase.active_player : null,
            turn: snapshot.public.phase.kind === "playing" ? snapshot.public.phase.turn : null,
            remaining: snapshot.public.remaining_tiles,
            rackCount: snapshot.own_rack.tiles.length,
            tileIds: snapshot.own_rack.tiles.map((tile) => tile.id),
            recipient: snapshot.own_rack.player_id,
            board: snapshot.public.board,
            scores: snapshot.public.players,
          };
        }
      }
      return frame;
    };
  });
}

test("board recovers a lost creation response with one game, then joins its real canonical route", async ({
  page,
  request,
}) => {
  await expect.poll(async () => (await request.get(`${fixture.status}/readyz`)).status()).toBe(200);
  const bodies: string[] = [];
  let firstMatch = "";
  await page.route(`${fixture.api}/games`, async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    bodies.push(route.request().postData() ?? "");
    if (bodies.length === 1) {
      const response = await route.fetch();
      expect(response.status()).toBe(200);
      firstMatch = z.object({ matchId: z.string() }).parse(await response.json()).matchId;
      await route.abort("failed");
    } else {
      await route.continue();
    }
  });
  const certificate = fixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  await page.goto(
    `./?server=${encodeURIComponent(fixture.endpoint)}&api=${encodeURIComponent(fixture.api)}&certificate=${certificate}`,
  );
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("response was interrupted");
  await page.getByRole("button", { name: "Retry creation", exact: true }).click();
  const invite = page.getByRole("link", { name: "Join this game", exact: true });
  await expect(invite).toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: "Connected" })).toContainText(
    "Connected",
  );
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
  const href = await invite.getAttribute("href");
  const pixels = await page.evaluate(async () => {
    const svg = document.querySelector('svg[aria-label="Scan to join this game"]');
    if (!(svg instanceof SVGSVGElement)) {
      throw new Error("Missing invite QR");
    }
    const image = new Image();
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(svg))}`;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const drawing = canvas.getContext("2d");
    if (drawing === null) {
      throw new Error("Missing canvas context");
    }
    drawing.drawImage(image, 0, 0);
    return {
      width: canvas.width,
      height: canvas.height,
      data: Array.from(drawing.getImageData(0, 0, canvas.width, canvas.height).data),
    };
  });
  expect(jsQR(new Uint8ClampedArray(pixels.data), pixels.width, pixels.height)?.data).toBe(href);
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.getByRole("button", { name: "Copy player link", exact: true }).click();
  await expect(page.getByText("Player link copied.", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(href);
  expect(href).not.toBeNull();
  const publicId = new URL(href ?? "").searchParams.get("match");
  expect(publicId).toBe(firstMatch);
  expect(new URL(page.url()).searchParams.get("match")).toBe(firstMatch);
  expect(bodies).toHaveLength(2);
  expect(bodies[0]).toBe(bodies[1]);
  const status = await (await request.get(`${fixture.status}/status`)).json();
  expect(status.capacity.hostedMatches).toBe(1);
  const result = await page.evaluate(
    async ({ fixture, matchId }) => {
      const modulePath = "/scrabble/src/transport/browser-match.ts";
      const { BrowserMatch } = (await import(
        modulePath
      )) as typeof import("../src/transport/browser-match");
      const gamePath = "/scrabble/src/game-protocol.ts";
      const { decodePlayerSnapshot } = (await import(
        gamePath
      )) as typeof import("../src/game-protocol");
      return new Promise<{ phase: string; gameId: string }>((resolve, reject) => {
        let playerId: string | undefined;
        const client = new BrowserMatch({
          endpoint: fixture.endpoint,
          matchId,
          serverCertificateHashes: [
            { algorithm: "sha-256", value: new Uint8Array(fixture.certificateHash).buffer },
          ],
          onState: (state) => {
            if (state.kind === "connected") {
              playerId = state.admission.playerId;
            }
          },
          onSnapshot: (snapshot) => {
            if (playerId === undefined) {
              return;
            }
            const projection = decodePlayerSnapshot(snapshot.payload, playerId);
            client.close();
            resolve({ phase: projection.public.phase.kind, gameId: projection.public.game_id });
          },
        });
        client.run().catch(reject);
      });
    },
    { fixture, matchId: publicId ?? "" },
  );
  expect(result.phase).toBe("lobby");
  expect(result.gameId).toMatch(/^[0-9]+$/);
  const storage = await page.evaluate(() => ({
    local: Object.keys(localStorage),
    purposes: Object.keys(sessionStorage).map((key) => key.split(":")[1]),
    privateUrl: /requestId|token|reconnect|sequence/.test(location.href),
  }));
  expect(storage).toEqual({ local: [], purposes: ["board"], privateUrl: false });
  expect(href).not.toContain("requestId");
  await page.reload();
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Create game", exact: true })).toHaveCount(0);
  expect(new URL(page.url()).searchParams.get("match")).toBe(firstMatch);
  const restoredStatus = await (await request.get(`${fixture.status}/status`)).json();
  expect(restoredStatus.capacity.hostedMatches).toBe(1);
});

test("lobby tracks joined players and confirms Start from authoritative snapshots", async ({
  page,
  context,
}) => {
  const certificate = fixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  await page.goto(
    `./?server=${encodeURIComponent(fixture.endpoint)}&api=${encodeURIComponent(fixture.api)}&certificate=${certificate}`,
  );
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  const start = page.getByRole("button", { name: "Start game", exact: true });
  await expect(start).toBeDisabled();
  const invite = await page
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  expect(invite).not.toBeNull();
  const guest = await context.newPage();
  await guest.goto(invite ?? "");
  await guest.getByLabel("Player name", { exact: true }).fill("Lin");
  await guest.getByRole("button", { name: "Join game", exact: true }).click();
  const roster = page.getByRole("list", { name: "Players and scores" });
  await expect(roster.getByRole("listitem")).toHaveCount(1);
  await expect(start).toBeDisabled();
  const secondGuest = await context.newPage();
  await secondGuest.goto(invite ?? "");
  await secondGuest.getByLabel("Player name", { exact: true }).fill("Max");
  await secondGuest.getByRole("button", { name: "Join game", exact: true }).click();
  await expect(roster.getByRole("listitem")).toHaveCount(2);
  await expect(start).toBeEnabled();
  await expect(guest.getByRole("button", { name: "Start game", exact: true })).toHaveCount(0);
  await expect(roster).not.toContainText("Player 1");
  await expect(roster).toContainText("Max");
  await expect(roster).toContainText("Lin");
  await expect(roster.getByRole("listitem").filter({ hasText: "Connected" })).toHaveCount(2);
  await page.evaluate(async () => {
    const modulePath = "/scrabble/src/transport/browser-match.ts";
    const { BrowserMatch } = (await import(
      modulePath
    )) as typeof import("../src/transport/browser-match");
    const original = Object.getOwnPropertyDescriptor(BrowserMatch.prototype, "sendCommand");
    if (original === undefined) {
      throw new Error("Missing command seam");
    }
    BrowserMatch.prototype.sendCommand = async () => 1;
    setTimeout(() => {
      Object.defineProperty(BrowserMatch.prototype, "sendCommand", original);
    }, 1000);
  });
  await start.click();
  await expect(page.getByRole("alert")).toContainText("Start was not confirmed", { timeout: 7000 });
  await expect(roster.getByRole("listitem")).toHaveCount(2);
  await expect(start).toBeEnabled();
  await start.click();
  await expect(page.getByText("Turn 1", { exact: true })).toBeVisible();
  await expect(guest.getByText("It is your turn.", { exact: true })).toBeVisible();
  await expect(secondGuest.getByText("Waiting for Lin.", { exact: true })).toBeVisible();
  await expect(start).toHaveCount(0);
  await guest.close();
  await expect(roster.getByRole("listitem").filter({ hasText: "Disconnected" })).toHaveCount(1);
  await expect(roster.getByRole("listitem")).toHaveCount(2);
  await secondGuest.close();
});

test("player invitations reject malformed and expired games before transport admission", async ({
  page,
}) => {
  const route = new URL("http://example.test/scrabble/");
  route.searchParams.set("view", "player");
  route.searchParams.set("server", fixture.endpoint);
  route.searchParams.set("api", fixture.api);
  route.searchParams.set("match", "../bad");
  await page.goto(`./${route.search}`);
  await expect(page.getByRole("alert")).toContainText("invalid game identifier");
  route.searchParams.set("match", `g_${"0".repeat(16)}_${"a".repeat(32)}`);
  await page.goto(`./${route.search}`);
  await expect(page.getByRole("alert")).toContainText("game has expired");
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("game has expired");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Create game", exact: true })).toHaveCount(0);
});

test("phone names preserve distinct identities across refresh and full-game failures", async ({
  page,
  browser,
}) => {
  const certificate = fixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  await page.goto(
    `./?server=${encodeURIComponent(fixture.endpoint)}&api=${encodeURIComponent(fixture.api)}&certificate=${certificate}`,
  );
  await observeProjection(page);
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  const href = await page
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  const firstContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const secondContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const thirdContext = await browser.newContext();
  const fourthContext = await browser.newContext();
  const fullContext = await browser.newContext();
  try {
    const first = await firstContext.newPage();
    const second = await secondContext.newPage();
    for (const phone of [first, second]) {
      await phone.goto(href ?? "");
      await observeProjection(phone);
      await expect(phone.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
      await phone.getByLabel("Player name", { exact: true }).fill("Ada");
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name: "Ada", exact: true })).toBeVisible();
    }
    const firstId = await first.getByTestId("player-identity").textContent();
    const secondId = await second.getByTestId("player-identity").textContent();
    expect(firstId).not.toBe(secondId);
    const roster = page.getByRole("list", { name: "Players and scores" });
    await expect(roster.getByRole("listitem").filter({ hasText: "Ada" })).toHaveCount(2);
    for (let refresh = 0; refresh < 2; refresh++) {
      await first.reload();
      await observeProjection(first);
      await expect(first.getByRole("heading", { name: "Ada", exact: true })).toBeVisible();
      await expect(first.getByTestId("player-identity")).toHaveText(firstId ?? "");
      await expect(first.getByLabel("Player name", { exact: true })).toHaveCount(0);
    }
    const privacy = await first.evaluate(() => {
      const keys = Object.keys(sessionStorage);
      const saved: unknown = JSON.parse(sessionStorage.getItem(keys[0] ?? "") ?? "null");
      return {
        local: Object.keys(localStorage),
        count: keys.length,
        fields: saved === null || typeof saved !== "object" ? [] : Object.keys(saved).sort(),
        privateUrl: /token|reconnect|sequence|rack|Ada/i.test(location.href),
        overflow: document.documentElement.scrollWidth > innerWidth,
      };
    });
    expect(privacy).toEqual({
      local: [],
      count: 1,
      fields: ["playerId", "route", "sequence", "token", "version"],
      privateUrl: false,
      overflow: false,
    });
    const third = await thirdContext.newPage();
    await third.goto(href ?? "");
    await observeProjection(third);
    await third.getByLabel("Player name", { exact: true }).fill("Lin");
    await third.getByRole("button", { name: "Join game", exact: true }).click();
    await expect(third.getByRole("heading", { name: "Lin", exact: true })).toBeVisible();
    const fourth = await fourthContext.newPage();
    await fourth.goto(href ?? "");
    await observeProjection(fourth);
    await fourth.getByLabel("Player name", { exact: true }).fill("Jo");
    await fourth.getByRole("button", { name: "Join game", exact: true }).click();
    await expect(fourth.getByRole("heading", { name: "Jo", exact: true })).toBeVisible();
    const full = await fullContext.newPage();
    await full.goto(href ?? "");
    await full.getByLabel("Player name", { exact: true }).fill("Fifth");
    await full.getByRole("button", { name: "Join game", exact: true }).click();
    await expect(full.getByRole("alert")).toContainText("game is full");
    await expect(full.getByTestId("player-identity")).toHaveCount(0);
    await expect(roster.getByRole("listitem")).toHaveCount(4);
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect(page.getByText("Turn 1", { exact: true })).toBeVisible();
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();
    for (const phone of [second, third, fourth]) {
      await expect(phone.getByText("Waiting for Ada.", { exact: true })).toBeVisible();
    }
    await expect.poll(() => page.evaluate(() => window.scrabbleObserved?.turn)).toBe("0");
    const board = await page.evaluate(() => window.scrabbleObserved);
    expect(board?.rackCount).toBe(0);
    expect(board?.tileIds).toEqual([]);
    expect(board?.order).not.toContain(board?.recipient);
    expect(board?.remaining).toBe(72);
    const tileIds = new Set<string>();
    for (const phone of [first, second, third, fourth]) {
      await expect.poll(() => phone.evaluate(() => window.scrabbleObserved?.turn)).toBe("0");
      const projection = await phone.evaluate(() => window.scrabbleObserved);
      expect(projection?.rackCount).toBe(7);
      expect(projection?.order).toEqual(board?.order);
      expect(projection?.active).toBe(board?.active);
      expect(projection?.turn).toBe(board?.turn);
      expect(projection?.remaining).toBe(board?.remaining);
      expect(projection?.gameId).toBe(board?.gameId);
      const rack = phone.getByRole("list", { name: "Your rack", exact: true });
      await expect(rack.getByRole("listitem")).toHaveCount(7);
      expect(
        await rack
          .getByRole("listitem")
          .evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-tile-id"))),
      ).toEqual(projection?.tileIds);
      await expect(rack).toHaveAttribute("aria-disabled", phone === first ? "false" : "true");
      await expect(rack.getByRole("button")).toHaveCount(7);
      if (phone !== first) {
        for (const button of await rack.getByRole("button").all()) {
          await expect(button).toBeDisabled();
        }
        await expect(
          phone.getByRole("button", { name: "Commit move", exact: true }),
        ).toBeDisabled();
      }
      await expect(phone.getByRole("textbox")).toHaveCount(0);
      await expect(
        phone.getByRole("list", { name: "Players and scores", exact: true }).getByRole("listitem"),
      ).toHaveCount(4);
      for (const id of projection?.tileIds ?? []) {
        expect(tileIds.has(id)).toBe(false);
        tileIds.add(id);
      }
    }
    expect(tileIds.size).toBe(28);
    const ownTiles = await first
      .getByRole("list", { name: "Your rack", exact: true })
      .getByRole("listitem")
      .evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-tile-id")));
    for (const phone of [second, third, fourth]) {
      const rendered = await phone
        .getByRole("list", { name: "Your rack", exact: true })
        .getByRole("listitem")
        .evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-tile-id")));
      expect(rendered.some((id) => ownTiles.includes(id))).toBe(false);
    }
    for (const viewport of [
      { width: 320, height: 568 },
      { width: 390, height: 844 },
      { width: 844, height: 390 },
    ]) {
      await first.setViewportSize(viewport);
      expect(await first.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(
        false,
      );
      const tiles = await first
        .getByRole("list", { name: "Your rack", exact: true })
        .getByRole("listitem")
        .evaluateAll((elements) =>
          elements.map((element) => {
            const bounds = element.getBoundingClientRect();
            return { width: bounds.width, x: bounds.x, right: bounds.right };
          }),
        );
      expect(
        tiles.every((tile) => tile.width >= 44 && tile.x >= 0 && tile.right <= viewport.width),
      ).toBe(true);
      await first.screenshot({
        path: `test-results/phone-rack-${viewport.width}.png`,
        fullPage: true,
      });
    }
    await page.reload();
    await expect(page.getByText("Turn 1", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Start game", exact: true })).toHaveCount(0);
    await full.getByRole("button", { name: "Retry player connection", exact: true }).click();
    await expect(full.getByRole("alert")).toContainText("game has already started");
    await first.reload();
    await expect(first.getByTestId("player-identity")).toHaveText(firstId ?? "");
    await expect(first.getByRole("heading", { name: "Ada", exact: true })).toBeVisible();
    await expect(first.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();
    const restoredRack = first.getByRole("list", { name: "Your rack", exact: true });
    await expect(restoredRack.getByRole("listitem")).toHaveCount(7);
    expect(
      await restoredRack
        .getByRole("listitem")
        .evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-tile-id"))),
    ).toEqual(ownTiles);
  } finally {
    await Promise.all([
      firstContext.close(),
      secondContext.close(),
      thirdContext.close(),
      fourthContext.close(),
      fullContext.close(),
    ]);
  }
});

test("active phones edit tentative moves and retain unconfirmed drafts through reconnect", async ({
  page,
  context,
}) => {
  const certificate = fixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  await page.goto(
    `./?server=${encodeURIComponent(fixture.endpoint)}&api=${encodeURIComponent(fixture.api)}&certificate=${certificate}`,
  );
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  const href = await page
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  const first = await context.newPage();
  const second = await context.newPage();
  try {
    for (const [index, phone] of [first, second].entries()) {
      await phone.goto(href ?? "");
      await phone.getByLabel("Player name", { exact: true }).fill(index === 0 ? "Ada" : "Lin");
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(
        phone.getByRole("heading", { name: index === 0 ? "Ada" : "Lin", exact: true }),
      ).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();
    const editor = first.getByRole("region", { name: "Move editor", exact: true });
    const rackButtons = first
      .getByRole("list", { name: "Your rack", exact: true })
      .getByRole("button");
    async function placeTile(index: number, row: number, column: number) {
      const tile = rackButtons.nth(index);
      const label = await tile.getAttribute("aria-label");
      await tile.click();
      await editor
        .getByRole("button", { name: new RegExp(`^Row ${row}, column ${column}:`) })
        .click();
      if (label?.startsWith("Blank tile")) {
        await editor
          .getByRole("group", { name: "Choose blank letter", exact: true })
          .getByRole("button", { name: "A", exact: true })
          .click();
      }
    }
    await placeTile(0, 8, 8);
    await expect(page.locator(".tentative-tile")).toHaveCount(1);
    await expect(page.getByRole("cell", { name: /^Row 8, column 8: Tentative/ })).toBeVisible();
    await expect(editor.getByRole("status")).toContainText("at least two letters");
    await placeTile(1, 8, 9);
    await expect(page.locator(".tentative-tile")).toHaveCount(2);
    await expect(editor.getByRole("status")).toContainText("Ready to commit");
    await expect(editor.locator(".tentative-square")).toHaveCount(2);
    await placeTile(1, 8, 10);
    await expect(page.getByRole("cell", { name: /^Row 8, column 10: Tentative/ })).toBeVisible();
    await expect(page.getByRole("cell", { name: /^Row 8, column 9: Normal square/ })).toBeVisible();
    await expect(editor.getByRole("status")).toContainText("Fill the gaps");
    await expect(editor.getByRole("button", { name: "Commit move", exact: true })).toBeDisabled();
    await placeTile(1, 9, 9);
    await expect(editor.getByRole("status")).toContainText("one row or column");
    await rackButtons.nth(1).click();
    await editor.getByRole("button", { name: "Return selected tile", exact: true }).click();
    await expect(rackButtons.nth(1)).toHaveAttribute("data-placed", "false");
    await editor.getByRole("button", { name: "Cancel move", exact: true }).click();
    await expect(editor.locator(".tentative-square")).toHaveCount(0);
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
    const firstFace = await rackButtons.nth(0).getAttribute("aria-label");
    const secondFace = await rackButtons.nth(1).getAttribute("aria-label");
    const firstLetter =
      firstFace?.startsWith("Blank tile") === true ? "A" : firstFace?.split(",")[0];
    const reverseAT = firstLetter === "A" && secondFace?.startsWith("T,") === true;
    await placeTile(reverseAT ? 1 : 0, 8, 8);
    await placeTile(reverseAT ? 0 : 1, 9, 8);
    await expect(editor.getByRole("status")).toContainText("Ready to commit");
    await editor.getByRole("button", { name: "Commit move", exact: true }).click();
    await expect(editor.getByRole("status")).toContainText("Waiting for move confirmation");
    await expect(editor.getByRole("status")).toContainText("Move was not confirmed", {
      timeout: 7000,
    });
    await expect(editor.locator(".tentative-square")).toHaveCount(2);
    await expect(page.locator(".letter-tile")).toHaveCount(0);
    await first.getByRole("button", { name: "Retry player connection", exact: true }).click();
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();
    await expect(editor.getByRole("button", { name: "Commit move", exact: true })).toBeEnabled();
    await expect(page.locator(".tentative-tile")).toHaveCount(2);
    await editor.getByRole("button", { name: "Cancel move", exact: true }).click();
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
  } finally {
    await first.close();
    await second.close();
  }
});

test("closing an active phone clears its tentative board without committing tiles", async ({
  page,
  context,
}) => {
  const certificate = fixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  await page.goto(
    `./?server=${encodeURIComponent(fixture.endpoint)}&api=${encodeURIComponent(fixture.api)}&certificate=${certificate}`,
  );
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  const href = await page
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  const first = await context.newPage();
  const second = await context.newPage();
  try {
    for (const [index, phone] of [first, second].entries()) {
      await phone.goto(href ?? "");
      await phone.getByLabel("Player name", { exact: true }).fill(index === 0 ? "Ada" : "Lin");
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(
        phone.getByRole("heading", { name: index === 0 ? "Ada" : "Lin", exact: true }),
      ).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();
    const tile = first
      .getByRole("list", { name: "Your rack", exact: true })
      .getByRole("button")
      .first();
    const blank = (await tile.getAttribute("aria-label"))?.startsWith("Blank tile");
    await tile.click();
    const editor = first.getByRole("region", { name: "Move editor", exact: true });
    await editor.getByRole("button", { name: /^Row 8, column 8:/ }).click();
    if (blank) {
      await editor
        .getByRole("group", { name: "Choose blank letter", exact: true })
        .getByRole("button", { name: "A", exact: true })
        .click();
    }
    await expect(page.locator(".tentative-tile")).toHaveCount(1);
    await first.close();
    await expect(page.getByRole("listitem").filter({ hasText: "Ada" })).toContainText(
      "Disconnected",
    );
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
    await expect(page.locator(".letter-tile")).toHaveCount(0);
    await expect(page.getByText("Turn 1", { exact: true })).toBeVisible();
    await expect(
      second.getByRole("list", { name: "Your rack", exact: true }).getByRole("listitem"),
    ).toHaveCount(7);
  } finally {
    await first.close();
    await second.close();
  }
});

test("an authoritative AT commit converges on the board and both private phones", async ({
  page,
  context,
}) => {
  const certificate = fixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  await page.goto(
    `./?server=${encodeURIComponent(fixture.endpoint)}&api=${encodeURIComponent(fixture.api)}&certificate=${certificate}`,
  );
  await observeProjection(page);
  await selectOpening(page);
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  const href = await page
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  const first = await context.newPage();
  const second = await context.newPage();
  try {
    for (const [index, phone] of [first, second].entries()) {
      await phone.goto(href ?? "");
      await observeProjection(phone);
      await phone.getByLabel("Player name", { exact: true }).fill(index === 0 ? "Ada" : "Lin");
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(
        phone.getByRole("heading", { name: index === 0 ? "Ada" : "Lin", exact: true }),
      ).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();
    const editor = first.getByRole("region", { name: "Move editor", exact: true });
    const rack = first.getByRole("list", { name: "Your rack", exact: true });
    await expect.poll(() => first.evaluate(() => window.scrabbleObserved?.rackCount)).toBe(7);
    await expect.poll(() => second.evaluate(() => window.scrabbleObserved?.rackCount)).toBe(7);
    const previousIds = await first.evaluate(() => window.scrabbleObserved?.tileIds ?? []);
    const otherIds = await second.evaluate(() => window.scrabbleObserved?.tileIds ?? []);
    const placedIds: string[] = [];
    for (const [index, letter] of ["A", "T"].entries()) {
      const tile = rack.getByRole("button", { name: new RegExp(`^${letter},`) }).first();
      placedIds.push(z.string().parse(await tile.locator("..").getAttribute("data-tile-id")));
      await tile.click();
      await editor
        .getByRole("button", { name: new RegExp(`^Row 8, column ${8 + index}:`) })
        .click();
    }
    await expect(page.locator(".tentative-tile")).toHaveCount(2);
    await editor.getByRole("button", { name: "Commit move", exact: true }).click();
    await expect(page.getByText("Turn 2", { exact: true })).toBeVisible();
    await expect(
      page.getByRole("cell", { name: /^Row 8, column 8: Committed A, 1 points/ }),
    ).toBeVisible();
    await expect(
      page.getByRole("cell", { name: /^Row 8, column 9: Committed T, 1 points/ }),
    ).toBeVisible();
    await expect(page.locator(".tentative-tile")).toHaveCount(0);
    await expect(first.getByText("Waiting for Lin.", { exact: true })).toBeVisible();
    await expect(second.getByText("It is your turn.", { exact: true })).toBeVisible();
    await expect(rack.getByRole("listitem")).toHaveCount(7);
    await expect(editor.locator(".tentative-square")).toHaveCount(0);
    for (const client of [page, first, second]) {
      await expect.poll(() => client.evaluate(() => window.scrabbleObserved?.turn)).toBe("1");
    }
    const board = await page.evaluate(() => window.scrabbleObserved);
    expect(board?.scores[0]?.score).toBe(4);
    expect(board?.remaining).toBe(84);
    expect(board?.tileIds).toEqual([]);
    const replacedIds = await first.evaluate(() => window.scrabbleObserved?.tileIds ?? []);
    expect(replacedIds).toHaveLength(7);
    expect(replacedIds.filter((id) => !previousIds.includes(id))).toHaveLength(2);
    expect(replacedIds.every((id) => !placedIds.includes(id) && !otherIds.includes(id))).toBe(true);
    expect(await second.evaluate(() => window.scrabbleObserved?.tileIds)).toEqual(otherIds);
    for (const phone of [first, second]) {
      const projection = await phone.evaluate(() => window.scrabbleObserved);
      expect(projection?.board).toEqual(board?.board);
      expect(projection?.scores).toEqual(board?.scores);
      expect(projection?.active).toEqual(board?.active);
      await expect(
        phone
          .getByRole("region", { name: "Move editor", exact: true })
          .locator(".committed-square"),
      ).toHaveCount(2);
      await expect(
        phone.getByRole("button", { name: "Retry player connection", exact: true }),
      ).toHaveCount(0);
    }
    // Observe beyond a heartbeat period: the old turn must not publish after commit.
    const committedTick = await first.evaluate(() => window.scrabbleObserved?.tick ?? "0");
    await expect
      .poll(() =>
        first.evaluate(
          (tick) => BigInt(window.scrabbleObserved?.tick ?? "0") >= BigInt(tick) + 12n,
          committedTick,
        ),
      )
      .toBe(true);
    await expect(
      first.getByRole("button", { name: "Retry player connection", exact: true }),
    ).toHaveCount(0);
    await first.reload();
    await observeProjection(first);
    await expect(first.getByText("Waiting for Lin.", { exact: true })).toBeVisible();
    await expect
      .poll(() => first.evaluate(() => window.scrabbleObserved?.tileIds))
      .toEqual(replacedIds);
    expect(
      await rack
        .getByRole("listitem")
        .evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-tile-id"))),
    ).toEqual(replacedIds);
    await expect(second.getByText("It is your turn.", { exact: true })).toBeVisible();
  } finally {
    await first.close();
    await second.close();
  }
});
