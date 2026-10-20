import { expect, test, type Page } from "@playwright/test";

// The expected URL states are independent of App's route selection logic.
const routes = [
  { url: "./?fixture=lobby", state: "lobby" },
  { url: "./?fixture=playing", state: "playing" },
  { url: "./?fixture=finished", state: "finished" },
  { url: "./?view=player&match=invalid", state: "invalid-player" },
] as const;

type RouteState = (typeof routes)[number]["state"];

async function expectRouteState(page: Page, state: RouteState) {
  if (state === "invalid-player") {
    await expect(page.getByRole("alert")).toBeVisible();
    await expect(page.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Create game", exact: true })).toHaveCount(0);
    return;
  }

  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
  await expect(page.locator(".fixture-note")).toBeVisible();
  if (state === "playing") {
    await expect(page.getByRole("status")).toHaveText("Ada's turn");
  } else if (state === "finished") {
    await expect(page.getByRole("status")).toHaveText("Winner: Ada");
  }
}

test("each URL state retains its content through the settings interaction and reload", async ({
  page,
}) => {
  for (const route of routes) {
    await page.goto(route.url);
    await expectRouteState(page, route.state);

    const url = page.url();
    const settings = page.getByRole("button", { name: "Settings", exact: true });
    const dialog = page.getByRole("dialog", { name: "Settings", exact: true });
    await expect(dialog).toHaveCount(0);
    await settings.click();
    await expect(dialog).toBeVisible();
    await expect(page).toHaveURL(url);

    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(settings).toBeFocused();
    await expect(page).toHaveURL(url);
    await expectRouteState(page, route.state);

    await page.reload();
    await expectRouteState(page, route.state);
    await expect(dialog).toHaveCount(0);
  }
});
