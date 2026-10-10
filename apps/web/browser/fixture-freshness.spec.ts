import { expect, test } from "@playwright/test";

import { gameFixture, selectOpening } from "./game-fixture";

test("creation fixture selection stays fresh after earlier tests consume the retry window", async ({
  page,
}) => {
  const query = new URLSearchParams({
    server: gameFixture.endpoint,
    api: gameFixture.api,
    certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
  });
  await page.goto(`./?${query}`);
  await selectOpening(page, {
    requestId: gameFixture.reconnectOpening.requestId,
    requestedAt: Math.floor(Date.now() / 1000) - 120,
  });
  await page.getByRole("button", { name: "Create game", exact: true }).click();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible({
    timeout: 2000,
  });
});
