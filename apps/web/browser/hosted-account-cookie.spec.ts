// The account cookie reaches the Scrabble API only when the board shares its site (#206).
// The built production bundle (deploy/client.json: https://moenarch.com/scrabble-api) is
// served from https://moenarch.com/scrabble/, next to the account service at /accounts/,
// as on the host. The account service's host-only cookie must then accompany the board's
// account request; from the GitHub Pages origin the same cookie is cross-site and stays
// behind. The server half (verifying the cookie) is covered by tests/account_linking.rs.
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { type BrowserContext } from "@playwright/test";

import { expect, test } from "./fixtures";
import { gameFixture } from "./game-fixture";

const build = fileURLToPath(new URL("../../../test-results/pages-moenarch/", import.meta.url));
const token = "mns_hosted-cookie-canary";
const accountId = "5f0c7b8e-0d3a-4a4e-9b1f-2a6c1c4e7d10";

/** Serves the built board below `/scrabble/` on `origin`. */
async function board(context: BrowserContext, origin: string) {
  await context.route(`${origin}/scrabble/**`, async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/scrabble/") {
      await route.fulfill({ path: join(build, "index.html") });
    } else if (/^\/scrabble\/assets\/[A-Za-z0-9_.-]+$/.test(path)) {
      await route.fulfill({ path: join(build, "assets", path.slice(path.lastIndexOf("/") + 1)) });
    } else {
      await route.fulfill({ status: 404 });
    }
  });
}

/**
 * moenarch.com's account page (which sets the session as the service does) and its
 * Scrabble API. Account requests record their cookie and answer as a server that
 * verified it would; every other API request goes to the real native fixture.
 */
async function moenarch(context: BrowserContext, fixtureOrigin: string, sent: (string | null)[]) {
  await context.route("https://moenarch.com/accounts/", (route) =>
    route.fulfill({
      contentType: "text/html",
      headers: {
        "Set-Cookie": `moenarch_session=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=600; Secure`,
      },
      body: "<!doctype html><title>Moenarch account</title><p>Signed in</p>",
    }),
  );
  await context.route("https://moenarch.com/scrabble-api/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const origin = (await request.headerValue("origin")) ?? "";
    const cors = {
      "Access-Control-Allow-Origin": origin,
      "Access-Control-Allow-Credentials": "true",
      "Access-Control-Allow-Headers": "Content-Type",
      Vary: "Origin",
    };
    if (url.pathname === "/scrabble-api/account/session") {
      if (request.method() === "OPTIONS") {
        await route.fulfill({ status: 204, headers: cors });
        return;
      }
      const cookie = (await request.headerValue("cookie")) ?? "";
      const session = /(?:^|;\s*)moenarch_session=([^;]*)/.exec(cookie)?.[1] ?? null;
      sent.push(session);
      if (origin !== "https://moenarch.com") {
        await route.fulfill({
          status: 403,
          headers: cors,
          json: { error: "guest-origin" },
        });
      } else if (session === token) {
        await route.fulfill({
          headers: cors,
          json: { version: 1, accountId, displayName: "Ada" },
        });
      } else {
        await route.fulfill({
          status: 401,
          headers: cors,
          json: { error: "signed-out", signIn: "https://moenarch.com/accounts/" },
        });
      }
      return;
    }
    const response = await route.fetch({
      url: gameFixture.api + url.pathname.slice("/scrabble-api".length) + url.search,
      headers: { ...request.headers(), origin: fixtureOrigin },
    });
    await route.fulfill({ response, headers: { ...response.headers(), ...cors } });
  });
}

test("the board on moenarch.com sends the account cookie to its API and shows the account", async ({
  browser,
  baseURL,
}) => {
  const context = await browser.newContext();
  const sent: (string | null)[] = [];
  try {
    await board(context, "https://moenarch.com");
    await moenarch(context, new URL(baseURL!).origin, sent);
    const page = await context.newPage();
    await page.goto("https://moenarch.com/accounts/");
    await page.goto("https://moenarch.com/scrabble/");
    const account = page.getByRole("region", { name: "Moenarch account" });
    await expect(account.getByText("Signed in as Ada.", { exact: true })).toBeVisible();
    expect(sent).toContain(token);
  } finally {
    await context.close();
  }
});

test("the GitHub Pages demo cannot send the cookie cross-site and stays guest-only", async ({
  browser,
  baseURL,
}) => {
  const context = await browser.newContext();
  const sent: (string | null)[] = [];
  try {
    await board(context, "https://moritzbrantner.github.io");
    await moenarch(context, new URL(baseURL!).origin, sent);
    const page = await context.newPage();
    await page.goto("https://moenarch.com/accounts/");
    await page.goto("https://moritzbrantner.github.io/scrabble/");
    await expect.poll(() => sent.length).toBeGreaterThan(0);
    expect(sent).not.toContain(token);
    await expect(page.getByRole("region", { name: "Moenarch account" })).toHaveCount(0);
  } finally {
    await context.close();
  }
});
