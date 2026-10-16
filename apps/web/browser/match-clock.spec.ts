// Independent #99 acceptance, authored on d701266 before production clock UI/forfeit edits.
// Uses the actual React application and native hosted WebTransport service, without clock overrides.
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { z } from "zod";

import { gameFixture } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

function entry() {
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`;
}
const enable = (page: Page) => page.getByRole("checkbox", { name: "Enable clocks", exact: true });
const model = (page: Page) => page.getByRole("combobox", { name: "Clock model", exact: true });
const consequence = (page: Page) =>
  page.getByRole("combobox", { name: "Timeout consequence", exact: true });
const clocks = (page: Page) => page.getByRole("region", { name: "Match clocks", exact: true });
const wireClock = z.object({
  players: z.array(z.object({ player_id: z.string(), remaining_ms: z.number() })),
  turn_started_at_ms: z.number().nullable(),
  expires_at_ms: z.number().nullable(),
});
async function configure(page: Page, kind: "cumulative" | "per_turn", outcome: "pass" | "forfeit") {
  await enable(page).check();
  await model(page).selectOption(kind);
  await consequence(page).selectOption(outcome);
  // Explicit acceptance inputs, never a product preset. Seconds keep setup understandable.
  if (kind === "cumulative") {
    await page.getByRole("spinbutton", { name: "Initial time (seconds)", exact: true }).fill("2");
    await page
      .getByRole("spinbutton", { name: "Turn-start increment (seconds)", exact: true })
      .fill("1");
  } else {
    await page.getByRole("spinbutton", { name: "Time per turn (seconds)", exact: true }).fill("3");
  }
}

test("each fresh creation is untimed and enabling clocks invents no timed selections", async ({
  page,
}) => {
  const requests: unknown[] = [];
  page.on("request", (request) => {
    if (request.url() === `${gameFixture.api}/games` && request.method() === "POST") {
      requests.push(request.postDataJSON());
    }
  });
  await page.goto(entry());
  await expect(enable(page)).not.toBeChecked();
  await enable(page).check();
  await expect(model(page)).toHaveValue("");
  await expect(consequence(page)).toHaveValue("");
  await model(page).selectOption("cumulative");
  await expect(
    page.getByRole("spinbutton", { name: "Initial time (seconds)", exact: true }),
  ).toHaveValue("");
  await expect(
    page.getByRole("spinbutton", { name: "Turn-start increment (seconds)", exact: true }),
  ).toHaveValue("");
  await consequence(page).selectOption("pass");
  await page.getByRole("spinbutton", { name: "Initial time (seconds)", exact: true }).fill("2");
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText(/positive.*increment|increment.*positive/i);
  expect(requests).toHaveLength(0);
  await page
    .getByRole("spinbutton", { name: "Turn-start increment (seconds)", exact: true })
    .fill("0");
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  expect(requests).toHaveLength(0);
  await page
    .getByRole("spinbutton", { name: "Turn-start increment (seconds)", exact: true })
    .fill("1");
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  expect(z.object({ timeControl: z.unknown() }).parse(requests[0]).timeControl).toEqual({
    clock: { kind: "cumulative", initial_ms: 2000, increment_ms: 1000 },
    on_expiry: "pass",
  });
  await page.goto(entry());
  await expect(enable(page)).not.toBeChecked();
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
  expect(z.record(z.string(), z.unknown()).parse(requests[1])["timeControl"]).toBeUndefined();
});

for (const kind of ["cumulative", "per_turn"] as const) {
  for (const outcome of ["pass", "forfeit"] as const) {
    test(`${kind} with ${outcome}: real setup, shared board, private controller and authoritative expiration`, async ({
      page,
      browser,
    }, testInfo) => {
      const board = await observeSnapshots(page, "1", true);
      await page.goto(entry());
      await configure(page, kind, outcome);
      await page.getByRole("button", { name: "Create game", exact: true }).click();
      const invite = page.getByRole("link", { name: "Join this game", exact: true });
      await expect(invite).toBeVisible();
      const href = await invite.getAttribute("href");
      expect(href).not.toBeNull();
      const contexts = await Promise.all([browser.newContext(), browser.newContext()]);
      try {
        const phones = await Promise.all(contexts.map((context) => context.newPage()));
        const readers = await Promise.all(
          phones.map((phone, index) => observeSnapshots(phone, String(index + 2))),
        );
        for (const [index, phone] of phones.entries()) {
          await phone.setViewportSize({ width: 390, height: 844 });
          await phone.goto(href!);
          await phone.getByLabel("Player name", { exact: true }).fill(index === 0 ? "Ada" : "Lin");
          await phone.getByRole("button", { name: "Join game", exact: true }).click();
        }
        await page.getByRole("button", { name: "Start game", exact: true }).click();
        await expect.poll(() => board()?.public.phase.kind).toBe("playing");
        const started = board();
        expect(started).toBeDefined();
        const initial = wireClock.parse(
          z.object({ clock: wireClock }).parse(started?.public).clock,
        );
        expect(initial.expires_at_ms! - initial.turn_started_at_ms!).toBe(3000);
        for (const target of [page, phones[0]!]) {
          await expect(clocks(target)).toBeVisible();
          await expect(clocks(target)).toContainText(
            kind === "cumulative" ? /cumulative/i : /per.turn/i,
          );
          await expect(clocks(target)).toContainText(
            outcome === "pass" ? /automatic pass/i : /forfeit/i,
          );
          await expect(target.getByRole("button", { name: /pause|resume clock/i })).toHaveCount(0);
        }
        const activeTimer = page.getByRole("timer", { name: "Ada remaining time", exact: true });
        const inactiveTimer = page.getByRole("timer", { name: "Lin remaining time", exact: true });
        const activeText = await activeTimer.textContent();
        const inactiveText = await inactiveTimer.textContent();
        await expect.poll(() => activeTimer.textContent(), { timeout: 1800 }).not.toBe(activeText);
        await expect(inactiveTimer).toHaveText(inactiveText ?? "");
        expect(board()?.own_rack.tiles).toEqual([]);
        await expect.poll(() => readers[0]?.()?.own_rack.tiles.length).toBeGreaterThan(0);
        const rack = readers[0]?.()?.own_rack;
        await testInfo.attach("desktop-clock", {
          body: await page.screenshot(),
          contentType: "image/png",
        });
        await testInfo.attach("phone-clock", {
          body: await phones[0]!.screenshot(),
          contentType: "image/png",
        });
        await expect
          .poll(() => board()?.public.history.length, { timeout: 8000 })
          .toBeGreaterThan(0);
        const result = board();
        expect(result?.public.history[0]?.player_id).toBe("2");
        expect(result?.public.history).toHaveLength(1);
        if (outcome === "pass") {
          expect(result?.public.phase).toEqual({ kind: "playing", active_player: "3", turn: "1" });
          expect(result?.public.history[0]?.action.kind).toBe("pass");
        } else {
          expect(result?.public.phase).toEqual({ kind: "finished", winners: ["3"] });
          await expect(
            page
              .getByRole("list", { name: "Final standings", exact: true })
              .getByRole("listitem")
              .first(),
          ).toContainText("Lin");
        }
        await expect(page.getByRole("region", { name: "Turn history", exact: true })).toContainText(
          /timed out|time expired|automatic pass|forfeit/i,
        );
        await expect.poll(() => readers[0]?.()?.public.history.length).toBe(1);
        expect(readers[0]?.()?.own_rack).toEqual(rack);
        // Reload the expired player's controller: no clock reset or repeated consequence.
        await phones[0]!.reload();
        await expect.poll(() => readers[0]?.()?.public.history.length).toBeGreaterThanOrEqual(1);
        expect(readers[0]?.()?.public.history[0]).toEqual(result?.public.history[0]);
        if (outcome === "forfeit") {
          const pass = phones[0]!.getByRole("button", { name: "Pass turn", exact: true });
          if (await pass.count()) {
            await expect(pass).toBeDisabled();
          }
        }
        expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
        expect((await new AxeBuilder({ page: phones[0]! }).analyze()).violations).toEqual([]);
      } finally {
        await Promise.all(contexts.map((context) => context.close()));
      }
    });
  }
}
