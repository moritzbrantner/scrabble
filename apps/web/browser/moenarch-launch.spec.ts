import { expect, test } from "@playwright/test";

import { gameFixture as fixture, selectOpening } from "./game-fixture";

const sessionId = "0123ABCDEFGH";

test("session-only and ambiguous launcher URLs cannot silently create Scrabble matches", async ({
  page,
}) => {
  await page.goto(`./?launch=moenarch&session=${sessionId}`);
  await expect(page.getByRole("alert")).toContainText("has no Scrabble match");
  await expect(page.getByRole("button", { name: "Create game", exact: true })).toHaveCount(0);
  await page.goto(`./?launch=moenarch&session=${sessionId}&session=${sessionId}`);
  await expect(page.getByRole("alert")).toContainText("Invalid moenarch.com session link");
  await expect(page.getByRole("button", { name: "Create game", exact: true })).toHaveCount(0);
});

test("explicit moenarch match links join and resume the same real Rust match across browsers", async ({
  page,
  browser,
}) => {
  const certificate = fixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  await page.goto(
    `./?server=${encodeURIComponent(fixture.endpoint)}&api=${encodeURIComponent(fixture.api)}&certificate=${certificate}`,
  );
  await selectOpening(page);
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByText(/^Connected · Player/)).toBeVisible();
  const launchBoard = new URL(page.url());
  const matchId = launchBoard.searchParams.get("match");
  expect(matchId).toMatch(/^b_[0-9a-f]{16}_[0-9a-f]{32}$/);
  launchBoard.searchParams.set("launch", "moenarch");
  launchBoard.searchParams.set("session", sessionId);
  // Board authority stays in this tab's private reconnect capsule, never in its URL.
  await page.goto(launchBoard.href);
  await expect(page.getByText(/^Connected · Player/)).toBeVisible();
  const invitation = page.getByRole("link", { name: "Join this game", exact: true });
  const href = await invitation.getAttribute("href");
  expect(href).not.toBeNull();
  const invite = new URL(href ?? "");
  expect(invite.searchParams.get("match")).toBe(matchId);
  expect(invite.searchParams.get("launch")).toBe("moenarch");
  expect(invite.searchParams.get("session")).toBe(sessionId);
  expect(invite.searchParams.get("view")).toBe("player");
  expect(href).not.toMatch(/requestId|reconnect|token|secret/);

  const firstContext = await browser.newContext();
  const secondContext = await browser.newContext();
  try {
    const first = await firstContext.newPage();
    const second = await secondContext.newPage();
    for (const [index, phone] of [first, second].entries()) {
      await phone.goto(invite.href);
      await phone.getByLabel("Player name", { exact: true }).fill(index === 0 ? "Ada" : "Lin");
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(
        phone.getByRole("heading", { name: index === 0 ? "Ada" : "Lin", exact: true }),
      ).toBeVisible();
    }
    const firstIdentity = await first.getByTestId("player-identity").textContent();
    const secondIdentity = await second.getByTestId("player-identity").textContent();
    expect(firstIdentity).not.toBe(secondIdentity);
    const roster = page.getByRole("list", { name: "Players and scores" });
    await expect(roster.getByRole("listitem")).toHaveCount(2);
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect(first.getByText("It is your turn.", { exact: true })).toBeVisible();

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
    await editor.getByRole("button", { name: "Commit move", exact: true }).click();
    await expect(page.getByText("Turn 2", { exact: true })).toBeVisible();
    await expect(second.getByText("It is your turn.", { exact: true })).toBeVisible();

    await first.reload();
    await expect(first.getByRole("heading", { name: "Ada", exact: true })).toBeVisible();
    await expect(first.getByTestId("player-identity")).toHaveText(firstIdentity ?? "");
    const restoredRack = first.getByRole("list", { name: "Your rack", exact: true });
    await expect(restoredRack.getByRole("listitem")).toHaveCount(7);
    await page.reload();
    await expect(page.getByText("Turn 2", { exact: true })).toBeVisible();
    await expect(roster.getByRole("listitem")).toHaveCount(2);
    expect(new URL(page.url()).searchParams.get("session")).toBe(sessionId);
    expect(new URL(first.url()).searchParams.get("match")).toBe(matchId);
  } finally {
    await firstContext.close();
    await secondContext.close();
  }
});
