import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { z } from "zod";
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
  await expect(page.getByRole("status")).toContainText("Connected");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
  const href = await invite.getAttribute("href");
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
  const roster = page.getByRole("list", { name: "Players and scores" });
  await expect(roster.getByRole("listitem")).toHaveCount(2);
  await expect(start).toBeEnabled();
  await expect(guest.getByRole("button", { name: "Start game", exact: true })).toBeDisabled();
  await expect(roster).toContainText("Player 1");
  await expect(roster).toContainText("Player 2");
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
  await expect(guest.getByText("Turn 1", { exact: true })).toBeVisible();
  await expect(start).toHaveCount(0);
  await guest.close();
  await expect(roster.getByRole("listitem").filter({ hasText: "Disconnected" })).toHaveCount(1);
  await expect(roster.getByRole("listitem")).toHaveCount(2);
});
