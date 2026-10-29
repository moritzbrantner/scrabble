import { expect, test, type Page } from "@playwright/test";
import { z } from "zod";

import { gameFixture } from "./game-fixture";

/**
 * #209: a backgrounded tab or switched-away phone app is hidden and often loses its network.
 * `unreachable` sends new sessions to a closed port, like a phone whose radio is off; `silent`
 * stops datagram delivery on a live session, like a frozen page whose session the network dropped.
 */
async function controllableNetwork(page: Page) {
  await page.addInitScript(() => {
    const NativeTransport = WebTransport;
    const state = { unreachable: false, silent: false, connections: 0 };
    const live: WebTransport[] = [];
    Object.assign(window, { scrabbleNetwork: state });
    window.addEventListener("test-drop-transport", () => {
      for (const transport of live.splice(0)) {
        transport.close();
      }
    });
    window.WebTransport = class extends NativeTransport {
      constructor(url: string | URL, options?: WebTransportOptions) {
        super(state.unreachable ? "https://127.0.0.1:9/unreachable" : url, options);
        state.connections += 1;
        state.silent = false; // A fresh session is a fresh network path.
        document.documentElement.dataset.connections = String(state.connections);
        live.push(this);
        const reader = this.datagrams.readable.getReader();
        const readable = new ReadableStream<Uint8Array>({
          async pull(controller) {
            while (true) {
              const result = await reader.read();
              if (result.done) {
                controller.close();
                return;
              }
              if (!state.silent) {
                controller.enqueue(result.value);
                return;
              }
            }
          },
          cancel: (reason) => reader.cancel(reason),
        });
        Object.defineProperty(this.datagrams, "readable", { value: readable });
      }
    };
  });
}

async function network(page: Page, change: { unreachable?: boolean; silent?: boolean }) {
  await page.evaluate((next) => {
    const state = (window as unknown as { scrabbleNetwork: Record<string, unknown> })
      .scrabbleNetwork;
    Object.assign(state, next);
  }, change);
}

async function visibility(page: Page, state: "hidden" | "visible") {
  await page.evaluate((next) => {
    Object.defineProperty(document, "visibilityState", { value: next, configurable: true });
    Object.defineProperty(document, "hidden", { value: next === "hidden", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
  }, state);
}

test("background interruptions resume creation, board and phone without a new game or seat", async ({
  page,
  browser,
}) => {
  test.setTimeout(60_000);
  const phones = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const ada = await phones.newPage();
  const lin = await phones.newPage();
  for (const client of [page, ada, lin]) {
    await controllableNetwork(client);
  }
  const bodies: string[] = [];
  let created = "";
  await page.route(`${gameFixture.api}/games`, async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    bodies.push(route.request().postData() ?? "");
    if (bodies.length === 1) {
      // The phone switched apps mid-create: the server created the game, the response is lost.
      const response = await route.fetch();
      created = z.object({ matchId: z.string() }).parse(await response.json()).matchId;
      await route.abort("failed");
    } else {
      await route.continue();
    }
  });
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  try {
    await page.goto(
      `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`,
    );
    await visibility(page, "hidden");
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("response was interrupted");
    await visibility(page, "visible");
    const invite = page.getByRole("link", { name: "Join this game", exact: true });
    await expect(invite).toBeVisible();
    // Returning retried the retained intent once: the same game, never a second one.
    expect(bodies).toHaveLength(2);
    expect(bodies[1]).toBe(bodies[0]);
    expect(new URL(page.url()).searchParams.get("match")).toBe(created);
    const href = await invite.getAttribute("href");
    for (const [phone, name] of [
      [ada, "Ada"],
      [lin, "Lin"],
    ] as const) {
      await phone.goto(href ?? "");
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    const rack = ada.getByRole("list", { name: "Your rack", exact: true });
    await expect(rack).toBeVisible();
    const tiles = await rack.innerText();

    for (const [client, player] of [
      [ada, "2"],
      [page, "1"],
    ] as const) {
      // Hidden with the network gone for longer than the whole foreground retry budget.
      await visibility(client, "hidden");
      await network(client, { unreachable: true });
      await client.evaluate(() => window.dispatchEvent(new Event("test-drop-transport")));
      await expect(
        client.getByRole("status").filter({ hasText: "Connection paused" }),
      ).toBeVisible();
      await client.waitForTimeout(6000);
      await expect(client.getByText("Reconnect attempts have stopped")).toHaveCount(0);
      await network(client, { unreachable: false });
      await visibility(client, "visible");
      await expect(client.getByText(`Connected · Player ${player}`, { exact: true })).toBeVisible();
    }
    expect(await rack.innerText()).toBe(tiles);

    // Back from a freeze to a session the network silently dropped: recycled, same identity.
    const before = Number(await ada.evaluate(() => document.documentElement.dataset.connections));
    await visibility(ada, "hidden");
    await network(ada, { silent: true });
    await visibility(ada, "visible");
    await expect
      .poll(() => ada.evaluate(() => Number(document.documentElement.dataset.connections)))
      .toBe(before + 1);
    await expect(ada.getByText("Connected · Player 2", { exact: true })).toBeVisible();
    expect(await rack.innerText()).toBe(tiles);
    await expect(ada.getByRole("button", { name: "Join game", exact: true })).toHaveCount(0);
  } finally {
    await phones.close();
  }
});
