import { readFile } from "node:fs/promises";
import { extname, join } from "node:path";

import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { z } from "zod";

import { gameFixture as fixture, selectOpening } from "../browser/game-fixture";

const configuration = z
  .object({
    SCRABBLE_ACCEPTANCE_SETUP_API: z.url(),
    SCRABBLE_ACCEPTANCE_SETUP_WEB: z.string(),
    SCRABBLE_ACCEPTANCE_PAGES: z.string(),
  })
  .parse(process.env);
const publicMatch = z.object({
  application: z.literal("scrabble"),
  matchId: z.string().regex(/^b_[0-9a-f]{16}_[0-9a-f]{32}$/),
});

function contentType(path: string) {
  const extension = extname(path);
  if (extension === ".html") {
    return "text/html";
  }
  if (extension === ".js") {
    return "text/javascript";
  }
  if (extension === ".css") {
    return "text/css";
  }
  if (extension === ".svg") {
    return "image/svg+xml";
  }
  return "application/octet-stream";
}

async function staticAsset(root: string, path: string) {
  const safe = path.split("/").filter(Boolean);
  if (safe.some((part) => part === ".." || part.includes("\\"))) {
    throw new Error("Invalid acceptance asset path");
  }
  return readFile(join(root, ...safe));
}

// This gateway changes location, not semantics: assets are the actual built
// applications, and HTTP requests are forwarded to the actual Rust processes.
// No match, game, join, snapshot, reconnect or signaling response is mocked.
async function gateway(context: BrowserContext, onCreation: () => void) {
  await context.route("https://moenarch.com/**", async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.startsWith("/lobbies") || url.pathname === "/health") {
      const response = await route.fetch({
        url: configuration.SCRABBLE_ACCEPTANCE_SETUP_API + url.pathname + url.search,
      });
      await route.fulfill({ response });
    } else if (url.pathname.startsWith("/scrabble-api/")) {
      if (url.pathname === "/scrabble-api/games" && route.request().method() === "POST") {
        onCreation();
      }
      const response = await route.fetch({
        url: fixture.api + url.pathname.slice("/scrabble-api".length) + url.search,
      });
      await route.fulfill({ response });
    } else {
      const path = url.pathname === "/" ? "/index.html" : url.pathname;
      const body = await staticAsset(configuration.SCRABBLE_ACCEPTANCE_SETUP_WEB, path);
      await route.fulfill({ body, contentType: contentType(path) });
    }
  });
  await context.route("https://moritzbrantner.github.io/scrabble/**", async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.slice("/scrabble/".length) || "index.html";
    await route.fulfill({
      body: await staticAsset(configuration.SCRABBLE_ACCEPTANCE_PAGES, path),
      contentType: contentType(path),
    });
  });
}

async function enterPlayer(launcher: Page, invite: string, name: string) {
  await launcher.goto(invite);
  const popup = launcher.waitForEvent("popup");
  await launcher.getByRole("button", { name: "Open Scrabble player", exact: true }).click();
  const phone = await popup;
  await phone.getByLabel("Player name", { exact: true }).fill(name);
  await phone.getByRole("button", { name: "Join game", exact: true }).click();
  await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
  return phone;
}

test("actual moenarch launcher creates one authoritative board, two players commit and reconnect after lobby expiry", async ({
  browser,
  request,
}) => {
  const hostContext = await browser.newContext();
  const firstContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const secondContext = await browser.newContext({ viewport: { width: 390, height: 844 } });
  let creations = 0;
  for (const context of [hostContext, firstContext, secondContext]) {
    // The gateway preserves public Pages origin while the native transport is
    // intentionally loopback. Grant this local-fixture permission only here;
    // normal TLS and pinned test-certificate verification remain active.
    await context.grantPermissions(["local-network-access"], {
      origin: "https://moritzbrantner.github.io",
    });
    await gateway(context, () => {
      creations++;
    });
  }
  const host = await hostContext.newPage();
  const firstLauncher = await firstContext.newPage();
  const secondLauncher = await secondContext.newPage();
  try {
    await host.goto("https://moenarch.com/scrabble.html");
    await host.getByRole("button", { name: "Create Scrabble lobby", exact: true }).click();
    const invitation = await host
      .getByRole("link", { name: "Invite link", exact: true })
      .getAttribute("href");
    const invite = new URL(z.string().parse(invitation), host.url());
    const session = z
      .string()
      .regex(/^[0123456789ABCDEFGHJKMNPQRSTVWXYZ]{12}$/)
      .parse(invite.searchParams.get("session"));
    expect([...invite.searchParams.keys()]).toEqual(["session"]);
    const boardEvent = host.waitForEvent("popup");
    await host.getByRole("button", { name: "Open Scrabble board", exact: true }).click();
    const board = await boardEvent;
    expect(new URL(board.url()).searchParams.get("view")).toBe("board");
    await expect(board.getByRole("button", { name: "Create game", exact: true })).toBeVisible();
    // Force the native refresh branch even in fast runs; the private target
    // created by the disposable wrapper must supply the executable.
    await selectOpening(board, { ...fixture.opening, requestedAt: 0 });
    await board.getByRole("button", { name: "Create game", exact: true }).click();
    await expect(board.getByRole("table", { name: "Scrabble board", exact: true })).toBeVisible();
    const bindingUrl = `${configuration.SCRABBLE_ACCEPTANCE_SETUP_API}/lobbies/${session}/game-reference`;
    await expect.poll(async () => (await request.get(bindingUrl)).status()).toBe(200);
    const binding = publicMatch.parse(await (await request.get(bindingUrl)).json());
    expect(new URL(board.url()).searchParams.get("match")).toBe(binding.matchId);
    await host.getByRole("button", { name: "Open Scrabble board", exact: true }).click();
    expect(hostContext.pages()).toHaveLength(2);
    expect(creations).toBe(1);
    const first = await enterPlayer(firstLauncher, invite.href, "Ada");
    const second = await enterPlayer(secondLauncher, invite.href, "Lin");
    for (const phone of [first, second]) {
      const url = new URL(phone.url());
      expect(url.searchParams.get("match")).toBe(binding.matchId);
      expect(url.searchParams.get("session")).toBe(session);
      expect(url.searchParams.get("view")).toBe("player");
      expect([...url.searchParams.keys()].sort()).toEqual(["launch", "match", "session", "view"]);
      await expect(phone.getByRole("table", { name: "Scrabble board", exact: true })).toHaveCount(
        0,
      );
      await expect(phone.getByRole("button", { name: "Start game", exact: true })).toHaveCount(0);
    }
    const identity = await first.getByTestId("player-identity").textContent();
    expect(identity).not.toBe(await second.getByTestId("player-identity").textContent());
    await expect(
      board.getByRole("list", { name: "Players and scores" }).getByRole("listitem"),
    ).toHaveCount(2);
    await board.getByRole("button", { name: "Start game", exact: true }).click();
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();
    const editor = first.getByRole("region", { name: "Move editor", exact: true });
    for (const [offset, letter] of ["A", "T"].entries()) {
      await first
        .getByRole("list", { name: "Your rack", exact: true })
        .getByRole("button", { name: new RegExp(`^${letter},`) })
        .first()
        .click();
      await editor
        .getByRole("button", { name: new RegExp(`^Row 8, column ${8 + offset}:`) })
        .click();
    }
    await editor.getByRole("button", { name: "Commit move", exact: true }).click();
    await expect(
      board.getByRole("cell", { name: /^Row 8, column 8: Committed A, 1 points/ }),
    ).toBeVisible();
    await expect(
      board.getByRole("cell", { name: /^Row 8, column 9: Committed T, 1 points/ }),
    ).toBeVisible();
    await expect(first.getByText("Waiting for Lin.", { exact: true })).toBeVisible();
    await expect(second.getByText("It is your turn.", { exact: true })).toBeVisible();
    await first.reload();
    await expect(first.getByTestId("player-identity")).toHaveText(identity ?? "");
    await expect(first.getByText("Waiting for Lin.", { exact: true })).toBeVisible();
    for (const phone of [first, second]) {
      await expect(
        phone
          .getByRole("region", { name: "Move editor", exact: true })
          .locator(".committed-square"),
      ).toHaveCount(2);
      await expect(
        phone.getByRole("list", { name: "Your rack", exact: true }).getByRole("listitem"),
      ).toHaveCount(7);
    }
    // A rematch is a new canonical game, not a replacement for the immutable
    // launcher binding. Carrying the old launch/session would rebind or reject it.
    for (let pass = 0; pass < 6; pass++) {
      const phone = pass % 2 === 0 ? second : first;
      await expect(phone.getByText("It is your turn.", { exact: true })).toBeVisible();
      await phone.getByRole("button", { name: "Pass turn", exact: true }).click();
      await phone.getByRole("button", { name: "Confirm pass", exact: true }).click();
    }
    await expect(board.getByText("Game finished", { exact: true })).toBeVisible();
    await board.getByRole("button", { name: "Start rematch", exact: true }).click();
    const rematchLink = board.getByRole("link", { name: "Join this game", exact: true });
    await expect(rematchLink).toBeVisible();
    const rematchUrl = new URL(z.string().parse(await rematchLink.getAttribute("href")));
    expect(rematchUrl.searchParams.get("match")).not.toBe(binding.matchId);
    expect([...rematchUrl.searchParams.keys()].sort()).toEqual(["match", "view"]);
    const boardParameters = new URL(board.url()).searchParams;
    expect(boardParameters.has("launch")).toBe(false);
    expect(boardParameters.has("session")).toBe(false);
    await expect(second.getByRole("button", { name: "Join rematch", exact: true })).toBeVisible();
    await second.getByRole("button", { name: "Join rematch", exact: true }).click();
    await expect(second.getByRole("heading", { name: "Lin", exact: true })).toBeVisible();
    const followed = new URL(second.url());
    expect(followed.searchParams.get("match")).toBe(rematchUrl.searchParams.get("match"));
    expect([...followed.searchParams.keys()].sort()).toEqual(["match", "view"]);
    expect(publicMatch.parse(await (await request.get(bindingUrl)).json())).toEqual(binding);
    // Let the actual short-lived setup lobby expire. The canonical Scrabble
    // invitation and saved player identity must survive independently of it.
    await expect
      .poll(async () => (await request.get(bindingUrl)).status(), {
        timeout: 70_000,
        intervals: [1000],
      })
      .toBe(404);
    await first.reload();
    await expect(first.getByTestId("player-identity")).toHaveText(identity ?? "");
    await expect(first.getByText("Game finished.", { exact: true })).toBeVisible();
    await expect(board.getByRole("button", { name: "Start game", exact: true })).toBeVisible();
    expect(creations).toBe(1);
    for (const page of [host, board, firstLauncher, first, secondLauncher, second]) {
      const clean = await page.evaluate(
        () =>
          !/token|nonce|requestId|participantToken|reconnect/i.test(location.href) &&
          !Object.keys(localStorage).some((key) =>
            /token|nonce|requestId|participantToken/i.test(key),
          ),
      );
      expect(clean).toBe(true);
    }
  } finally {
    await Promise.all([hostContext.close(), firstContext.close(), secondContext.close()]);
  }
});

for (const failure of ["unavailable transport", "unowned existing match"] as const) {
  test(`launcher binding remains absent for ${failure}`, async ({ browser, request }) => {
    const context = await browser.newContext();
    await context.grantPermissions(["local-network-access"], {
      origin: "https://moritzbrantner.github.io",
    });
    await gateway(context, () => {});
    if (failure === "unavailable transport") {
      await context.addInitScript(() => {
        // Deliberately unavailable browser transport, while real HTTP game
        // creation remains intact. No board-claim response is fabricated.
        Object.defineProperty(window, "WebTransport", { value: undefined });
      });
    }
    const host = await context.newPage();
    try {
      await host.goto("https://moenarch.com/scrabble.html");
      await host.evaluate(() => {
        document.documentElement.dataset.publicReports = "0";
        window.addEventListener("message", (event: MessageEvent<unknown>) => {
          if (
            event.origin === "https://moritzbrantner.github.io" &&
            event.data !== null &&
            typeof event.data === "object" &&
            "type" in event.data &&
            event.data.type === "scrabble-match-created"
          ) {
            document.documentElement.dataset.publicReports = String(
              Number(document.documentElement.dataset.publicReports) + 1,
            );
          }
        });
      });
      await host.getByRole("button", { name: "Create Scrabble lobby", exact: true }).click();
      const invite = new URL(
        z
          .string()
          .parse(
            await host.getByRole("link", { name: "Invite link", exact: true }).getAttribute("href"),
          ),
        host.url(),
      );
      const session = z.string().parse(invite.searchParams.get("session"));
      const popup = host.waitForEvent("popup");
      await host.getByRole("button", { name: "Open Scrabble board", exact: true }).click();
      const board = await popup;
      await expect(board.getByRole("button", { name: "Create game", exact: true })).toBeVisible();
      if (failure === "unavailable transport") {
        const creation = board.waitForResponse(
          (response) =>
            response.url() === "https://moenarch.com/scrabble-api/games" &&
            response.request().method() === "POST",
        );
        await board.getByRole("button", { name: "Create game", exact: true }).click();
        expect((await creation).status()).toBe(200);
        await expect(board.getByRole("alert")).toBeVisible();
      } else {
        const bytes = crypto.getRandomValues(new Uint8Array(16));
        const created = await request.post(`${fixture.api}/games`, {
          data: {
            version: 2,
            requestId: Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(""),
            requestedAt: Math.floor(Date.now() / 1000),
          },
        });
        expect(created.status()).toBe(200);
        const match = z.object({ matchId: z.string() }).parse(await created.json()).matchId;
        const url = new URL(board.url());
        url.searchParams.set("match", match);
        expect(
          await board.evaluate(
            () =>
              Object.keys(sessionStorage).filter((key) => key.startsWith("scrabble:board:")).length,
          ),
        ).toBe(0);
        await board.goto(url.href);
        await expect(board.getByRole("alert")).toContainText(
          /ownership|recover this board session/i,
        );
      }
      await host.evaluate(
        () =>
          new Promise<void>((resolve) => {
            const listener = (event: MessageEvent<unknown>) => {
              if (event.data === "review-acceptance-barrier") {
                window.removeEventListener("message", listener);
                resolve();
              }
            };
            window.addEventListener("message", listener);
            window.postMessage("review-acceptance-barrier", window.location.origin);
          }),
      );
      // The error UI establishes that the admission/ownership decision has run.
      // No pre-proof report may have irreversibly bound the host's lobby.
      const response = await request.get(
        `${configuration.SCRABBLE_ACCEPTANCE_SETUP_API}/lobbies/${session}/game-reference`,
      );
      expect(response.status()).toBe(409);
      expect(await response.json()).toEqual({
        error: { code: "game-not-bound", message: expect.any(String) },
      });
      expect(
        await host.evaluate(() => Number(document.documentElement.dataset.publicReports)),
      ).toBe(0);
    } finally {
      await context.close();
    }
  });
}

test("owned board reconnect re-reports its confirmed public binding", async ({
  browser,
  request,
}) => {
  const context = await browser.newContext();
  await context.grantPermissions(["local-network-access"], {
    origin: "https://moritzbrantner.github.io",
  });
  await gateway(context, () => {});
  const host = await context.newPage();
  try {
    await host.goto("https://moenarch.com/scrabble.html");
    await host.evaluate(() => {
      document.documentElement.dataset.publicReports = "0";
      window.addEventListener("message", (event: MessageEvent<unknown>) => {
        if (
          event.origin === "https://moritzbrantner.github.io" &&
          event.data !== null &&
          typeof event.data === "object" &&
          "type" in event.data &&
          event.data.type === "scrabble-match-created"
        ) {
          document.documentElement.dataset.publicReports = String(
            Number(document.documentElement.dataset.publicReports) + 1,
          );
        }
      });
    });
    await host.getByRole("button", { name: "Create Scrabble lobby", exact: true }).click();
    const invite = new URL(
      z
        .string()
        .parse(
          await host.getByRole("link", { name: "Invite link", exact: true }).getAttribute("href"),
        ),
      host.url(),
    );
    const session = z.string().parse(invite.searchParams.get("session"));
    const popup = host.waitForEvent("popup");
    await host.getByRole("button", { name: "Open Scrabble board", exact: true }).click();
    const board = await popup;
    await board.getByRole("button", { name: "Create game", exact: true }).click();
    await expect(board.getByRole("button", { name: "Start game", exact: true })).toBeVisible();
    const bindingUrl = `${configuration.SCRABBLE_ACCEPTANCE_SETUP_API}/lobbies/${session}/game-reference`;
    await expect.poll(async () => (await request.get(bindingUrl)).status()).toBe(200);
    const binding = publicMatch.parse(await (await request.get(bindingUrl)).json());
    const before = await host.evaluate(() =>
      Number(document.documentElement.dataset.publicReports),
    );
    await board.reload();
    await expect(board.getByRole("button", { name: "Start game", exact: true })).toBeVisible();
    await expect
      .poll(() => host.evaluate(() => Number(document.documentElement.dataset.publicReports)))
      .toBeGreaterThan(before);
    expect(publicMatch.parse(await (await request.get(bindingUrl)).json())).toEqual(binding);
    // The reload deliberately loses the private creation nonce. Rematch remains
    // the responsibility of the original creator document in the long journey.
  } finally {
    await context.close();
  }
});
