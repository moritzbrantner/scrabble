import { z } from "zod";

import { expect, test } from "./fixtures";
import { fixtureQuery, gameFixture } from "./game-fixture";

const message =
  "This link repeats a routing parameter. Ask for a corrected link with each parameter appearing only once.";

for (const key of [
  "match",
  "view",
  "lang",
  "server",
  "api",
  "certificate",
  "share",
  "launch",
  "session",
  "fixture",
]) {
  test(`rejects repeated ${key} before using either value`, async ({ page }) => {
    const query = fixtureQuery();
    query.set("view", "player");
    query.set("match", `g_${"0".repeat(16)}_${"a".repeat(32)}`);
    query.set("lang", "en");
    if (!query.has(key)) {
      query.set(key, "first");
    }
    query.append(key, "private-value-must-not-be-displayed");
    const requests: string[] = [];
    page.on("request", (request) => {
      if (["fetch", "xhr", "webtransport"].includes(request.resourceType())) {
        requests.push(request.url());
      }
    });
    const route = `./?${query}&unrelated=a&unrelated=b`;
    await page.goto(route);
    const original = page.url();
    await expect(page.getByRole("alert")).toHaveText(message);
    await expect(page.getByLabel("Player name", { exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Create game", exact: true })).toHaveCount(0);
    await expect(page.locator("main")).not.toContainText("private-value-must-not-be-displayed");
    expect(requests).toEqual([]);
    expect(page.url()).toBe(original);
    await page.reload();
    await expect(page.getByRole("alert")).toHaveText(message);
    expect(requests).toEqual([]);
  });
}

test("identical and empty decoded duplicates also reject", async ({ page }) => {
  for (const query of ["lang=en&lang=en", "match=&match=", "view=player&%76iew=player"]) {
    await page.goto(`./?${fixtureQuery()}&${query}`);
    await expect(page.getByRole("alert")).toHaveText(message);
  }
});

test("single routing keys and repeated unknown keys preserve a real join", async ({
  page,
  request,
}) => {
  const created = await request.post(`${gameFixture.api}/games`, {
    data: {
      version: 2,
      requestId: crypto.randomUUID().replaceAll("-", ""),
      requestedAt: Math.floor(Date.now() / 1000),
    },
  });
  expect(created.ok()).toBe(true);
  const body: unknown = await created.json();
  const { matchId } = z.object({ matchId: z.string() }).parse(body);
  const query = fixtureQuery();
  query.set("view", "player");
  query.set("match", matchId);
  query.set("lang", "de");
  query.append("unrelated", "a&b=c d");
  query.append("unrelated", "second");
  await page.goto(`./?${query}#kept`);
  await expect(page.getByLabel("Spielername", { exact: true })).toBeVisible();
  const original = page.url();
  expect(new URL(original).searchParams.getAll("unrelated")).toEqual(["a&b=c d", "second"]);
  await page.reload();
  await expect(page.getByLabel("Spielername", { exact: true })).toBeVisible();
  expect(page.url()).toBe(original);
});
