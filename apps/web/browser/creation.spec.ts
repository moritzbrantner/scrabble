import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { z } from "zod";
import jsQR from "jsqr";
const path = process.env.SCRABBLE_GAME_FIXTURE;
if (path === undefined) {
  throw new Error("Use bun run test:browser to provision Scrabble");
}
const fixture = z
  .strictObject({
    endpoint: z.string(),
    api: z.string(),
    status: z.string(),
    certificateHash: z.array(z.int().min(0).max(255)).length(32),
  })
  .parse(JSON.parse(readFileSync(path, "utf8")));

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
  expect(
    await page.evaluate(() => ({
      local: Object.keys(localStorage),
      session: Object.keys(sessionStorage),
      url: location.href,
    })),
  ).toEqual({ local: [], session: [], url: page.url() });
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
  await expect(roster.getByRole("listitem")).toHaveCount(2);
  await expect(start).toBeEnabled();
  await expect(guest.getByRole("button", { name: "Start game", exact: true })).toHaveCount(0);
  await expect(roster).toContainText("Player 1");
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
  await expect(guest.getByText("Waiting for Player 1.", { exact: true })).toBeVisible();
  await expect(start).toHaveCount(0);
  await guest.close();
  await expect(roster.getByRole("listitem").filter({ hasText: "Disconnected" })).toHaveCount(1);
  await expect(roster.getByRole("listitem")).toHaveCount(2);
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
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  const href = await page
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  const firstContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const secondContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const thirdContext = await browser.newContext();
  const fullContext = await browser.newContext();
  try {
    const first = await firstContext.newPage();
    const second = await secondContext.newPage();
    for (const phone of [first, second]) {
      await phone.goto(href ?? "");
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
    await third.getByLabel("Player name", { exact: true }).fill("Lin");
    await third.getByRole("button", { name: "Join game", exact: true }).click();
    await expect(third.getByRole("heading", { name: "Lin", exact: true })).toBeVisible();
    const full = await fullContext.newPage();
    await full.goto(href ?? "");
    await full.getByLabel("Player name", { exact: true }).fill("Fourth");
    await full.getByRole("button", { name: "Join game", exact: true }).click();
    await expect(full.getByRole("alert")).toContainText("game is full");
    await expect(full.getByTestId("player-identity")).toHaveCount(0);
    await expect(roster.getByRole("listitem")).toHaveCount(4);
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect(page.getByText("Turn 1", { exact: true })).toBeVisible();
    await expect(first.getByText("Waiting for Player 1.", { exact: true })).toBeVisible();
    await full.getByRole("button", { name: "Retry player connection", exact: true }).click();
    await expect(full.getByRole("alert")).toContainText("game has already started");
    await first.reload();
    await expect(first.getByTestId("player-identity")).toHaveText(firstId ?? "");
    await expect(first.getByRole("heading", { name: "Ada", exact: true })).toBeVisible();
    await expect(first.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
    await expect(first.getByText("Waiting for Player 1.", { exact: true })).toBeVisible();
  } finally {
    await Promise.all([
      firstContext.close(),
      secondContext.close(),
      thirdContext.close(),
      fullContext.close(),
    ]);
  }
});
