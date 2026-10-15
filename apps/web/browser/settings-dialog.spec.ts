// Independently authored acceptance for #95 (Decision Atlas 2.2, 2.3, 2.5) before implementation.
// Contract: docs/specs/settings-dialog.md.
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";

import { gameFixture } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

type Names = { title: string; close: string; language: string; appearance: string };
const english: Names = {
  title: "Settings",
  close: "Close",
  language: "Interface language",
  appearance: "Appearance",
};
const german: Names = {
  title: "Einstellungen",
  close: "Schließen",
  language: "Sprache der Oberfläche",
  appearance: "Darstellung",
};
const spanish: Names = {
  title: "Ajustes",
  close: "Cerrar",
  language: "Idioma de la interfaz",
  appearance: "Apariencia",
};
const desktop = { width: 1280, height: 900 };
const mobile = { width: 390, height: 844 };

const creationQuery = new URLSearchParams({
  server: gameFixture.endpoint,
  api: gameFixture.api,
  certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
});

const gear = (page: Page, names = english) =>
  page.getByRole("button", { name: names.title, exact: true });
const dialog = (page: Page, names = english) =>
  page.getByRole("dialog", { name: names.title, exact: true });
// Role queries exclude hidden elements, so a closed dialog repeating these selects stays unambiguous.
const language = (page: Page, names = english) =>
  page.getByRole("combobox", { name: names.language, exact: true });
const appearance = (page: Page, names = english) =>
  page.getByRole("combobox", { name: names.appearance, exact: true });

async function accessible(page: Page) {
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
}

async function attach(page: Page, testInfo: TestInfo, name: string) {
  await testInfo.attach(name, { body: await page.screenshot(), contentType: "image/png" });
}

async function box(locator: Locator) {
  const bounds = await locator.boundingBox();
  if (bounds === null) {
    throw new Error("Missing element bounds");
  }
  return bounds;
}

/** Decision 2.2/2.3 chrome: top-right gear plus directly visible language and appearance. */
async function expectChrome(page: Page, viewport: { width: number }, names = english) {
  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator("summary", { hasText: names.title })).toHaveCount(0);
  await expect(page.locator("details", { hasText: names.language })).toHaveCount(0);
  const button = gear(page, names);
  await expect(button).toBeVisible();
  await expect(button).toBeInViewport();
  expect(await button.locator("svg, img").count()).toBeGreaterThan(0);
  const bounds = await box(button);
  expect(bounds.width).toBeGreaterThanOrEqual(44);
  expect(bounds.height).toBeGreaterThanOrEqual(44);
  expect(bounds.y).toBeLessThanOrEqual(64);
  expect(bounds.x + bounds.width).toBeGreaterThanOrEqual(viewport.width - 96);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width);
  for (const select of [language(page, names), appearance(page, names)]) {
    await expect(select).toBeVisible();
    await expect(select).toBeInViewport();
    expect((await box(select)).height).toBeGreaterThanOrEqual(44);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
    viewport.width,
  );
}

async function focusInside(target: Locator) {
  return target.evaluate(
    (element) =>
      document.activeElement !== null &&
      document.activeElement !== document.body &&
      element.contains(document.activeElement),
  );
}

/** Opens with the gear, proves modal semantics and focus entry, and returns the dialog. */
async function openSettings(page: Page, names = english, via: "click" | "keyboard" = "click") {
  const url = page.url();
  if (via === "keyboard") {
    await gear(page, names).focus();
    await page.keyboard.press("Enter");
  } else {
    await gear(page, names).click();
  }
  const opened = dialog(page, names);
  await expect(opened).toBeVisible();
  expect(
    await opened.evaluate(
      (element) =>
        element.getAttribute("aria-modal") === "true" ||
        (element instanceof HTMLDialogElement && element.matches(":modal")),
    ),
  ).toBe(true);
  await expect(opened.getByRole("heading", { name: names.title, exact: true })).toBeVisible();
  await expect.poll(() => focusInside(opened)).toBe(true);
  expect(page.url()).toBe(url);
  return opened;
}

async function expectClosedWithFocusOnGear(page: Page, names = english) {
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(gear(page, names)).toBeFocused();
}

async function expectPresentationOnly(opened: Locator, names = english) {
  await expect(opened.getByRole("button", { name: names.close, exact: true })).toBeVisible();
  await expect(opened.getByRole("textbox")).toHaveCount(0);
  await expect(opened.getByRole("spinbutton")).toHaveCount(0);
  // Only the two interface preferences may appear as selects; no gameplay or rules settings.
  const allowed =
    (await opened.getByRole("combobox", { name: names.language, exact: true }).count()) +
    (await opened.getByRole("combobox", { name: names.appearance, exact: true }).count());
  await expect(opened.getByRole("combobox")).toHaveCount(allowed);
  // Decision 2.5: neither role's dialog becomes a place for private racks or editing controls.
  await expect(opened.getByRole("list", { name: "Your rack" })).toHaveCount(0);
  await expect(opened.getByRole("button", { name: "Commit move" })).toHaveCount(0);
}

/** Any repeated selector inside the dialog is bound to the one visible preferences state. */
async function expectRepeatedControlsInSync(page: Page, opened: Locator, names = english) {
  const values = await page.evaluate(
    (key) => JSON.parse(localStorage.getItem(key) ?? "null") as unknown,
    "scrabble:preferences:v1",
  );
  for (const [select, expected] of [
    [opened.getByRole("combobox", { name: names.language, exact: true }), "locale"],
    [opened.getByRole("combobox", { name: names.appearance, exact: true }), "theme"],
  ] as const) {
    if ((await select.count()) > 0 && values !== null && typeof values === "object") {
      await expect(select).toHaveValue(String((values as Record<string, unknown>)[expected]));
    }
  }
}

for (const viewport of [desktop, mobile]) {
  test(`gear and visible preferences appear on every route at ${viewport.width}x${viewport.height}`, async ({
    browser,
  }, testInfo) => {
    const context = await browser.newContext({ viewport });
    const page = await context.newPage();
    try {
      for (const [name, route] of [
        ["shared-board", "./?fixture=playing"],
        ["creation", `./?${creationQuery}`],
        ["error", "./?view=player&match=invalid"],
      ] as const) {
        await page.goto(route);
        await expect(page.locator("main").first()).toBeVisible();
        await expectChrome(page, viewport);
        await accessible(page);
        await attach(page, testInfo, `${name}-${viewport.width}-closed.png`);
        const opened = await openSettings(page);
        await expectPresentationOnly(opened);
        await accessible(page);
        await attach(page, testInfo, `${name}-${viewport.width}-open.png`);
        await page.keyboard.press("Escape");
        await expectClosedWithFocusOnGear(page);
      }
    } finally {
      await context.close();
    }
  });
}

test("gear chrome fits the narrowest supported phone without horizontal scrolling", async ({
  browser,
}) => {
  const viewport = { width: 320, height: 568 };
  const context = await browser.newContext({ viewport });
  const page = await context.newPage();
  try {
    await page.goto("./?fixture=playing");
    await expectChrome(page, viewport);
    const opened = await openSettings(page);
    const bounds = await box(opened);
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width);
    await opened.getByRole("button", { name: "Close", exact: true }).click();
    await expectClosedWithFocusOnGear(page);
  } finally {
    await context.close();
  }
});

test("settings dialog is modal, traps focus, closes by Escape or Close and restores focus", async ({
  browser,
}) => {
  const context = await browser.newContext({ viewport: desktop, reducedMotion: "reduce" });
  const page = await context.newPage();
  try {
    await page.goto("./?fixture=playing");
    const board = page.getByRole("table", { name: "Scrabble board" });
    await expect(board).toBeVisible();
    const before = await box(board);
    const url = page.url();

    const opened = await openSettings(page, english, "keyboard");
    // No inline expansion: the game remains in place behind the modal.
    expect(await box(board)).toEqual(before);
    expect(
      await opened.evaluate(
        (element) =>
          element
            .getAnimations({ subtree: true })
            .filter((animation) => animation.playState === "running").length,
      ),
    ).toBe(0);
    await expect(opened).toContainText("does not change the word list");
    await expectPresentationOnly(opened);
    for (const key of ["Tab", "Shift+Tab"]) {
      for (let step = 0; step < 12; step++) {
        await page.keyboard.press(key);
        expect(await focusInside(opened), `${key} ${step + 1} keeps focus in Settings`).toBe(true);
      }
    }
    await page.keyboard.press("Escape");
    await expectClosedWithFocusOnGear(page);
    expect(page.url()).toBe(url);
    expect(await box(board)).toEqual(before);

    await page.keyboard.press(" ");
    const again = dialog(page);
    await expect(again).toBeVisible();
    await again.getByRole("button", { name: "Close", exact: true }).click();
    await expectClosedWithFocusOnGear(page);
    expect(page.url()).toBe(url);
    expect(await page.evaluate(() => localStorage.length)).toBe(0);
  } finally {
    await context.close();
  }
});

test("gear, dialog and close action are translated and follow live language changes", async ({
  page,
}) => {
  await page.goto("./?fixture=playing&lang=de");
  await expect(page.locator("html")).toHaveAttribute("lang", "de");
  await expectChrome(page, desktop, german);
  let opened = await openSettings(page, german);
  await expect(opened.getByRole("button", { name: german.close, exact: true })).toBeVisible();
  await expect(opened).not.toContainText("does not change the word list");
  await opened.getByRole("button", { name: german.close, exact: true }).click();
  await expectClosedWithFocusOnGear(page, german);

  await language(page, german).selectOption("es");
  await expect(page.locator("html")).toHaveAttribute("lang", "es");
  expect(new URL(page.url()).searchParams.get("lang")).toBe("es");
  await expectChrome(page, desktop, spanish);
  opened = await openSettings(page, spanish);
  await expectRepeatedControlsInSync(page, opened, spanish);
  await page.keyboard.press("Escape");
  await expectClosedWithFocusOnGear(page, spanish);

  await page.goto("./?fixture=playing&lang=fr");
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  await expectChrome(page, desktop, english);
  await expect(language(page)).toHaveValue("en");
});

test("light and dark appearance visibly change the settings dialog", async ({ page }, testInfo) => {
  await page.goto("./?fixture=playing");
  const colours = async (theme: "light" | "dark") => {
    await appearance(page).selectOption(theme);
    if (theme === "dark") {
      await expect(page.locator("html")).toHaveClass("dark");
    } else {
      await expect(page.locator("html")).not.toHaveClass("dark");
    }
    const opened = await openSettings(page);
    await expectRepeatedControlsInSync(page, opened);
    await accessible(page);
    await attach(page, testInfo, `settings-${theme}.png`);
    const pair = await opened.evaluate((element) => {
      const style = getComputedStyle(element);
      return `${style.backgroundColor}|${style.color}`;
    });
    await page.keyboard.press("Escape");
    await expectClosedWithFocusOnGear(page);
    return pair;
  };
  const light = await colours("light");
  const dark = await colours("dark");
  expect(dark).not.toBe(light);
});

test("blocked storage during an open dialog stays perceivable", async ({ page }) => {
  await page.addInitScript(() => {
    Storage.prototype.setItem = () => {
      throw new Error("Storage disabled");
    };
  });
  await page.goto("./?fixture=playing");
  await appearance(page).selectOption("dark");
  const alert = page
    .getByRole("alert")
    .filter({ hasText: "Preferences could not be saved. They apply to this page." });
  await expect(alert).toBeVisible();
  const opened = await openSettings(page);
  const repeated = opened.getByRole("combobox", { name: "Appearance", exact: true });
  if ((await repeated.count()) > 0) {
    await repeated.selectOption("light");
    await expect(page.locator("html")).not.toHaveClass("dark");
    // The announcement must not sit behind the modal while it is open.
    const shown = page
      .getByRole("alert")
      .filter({ hasText: "Preferences could not be saved. They apply to this page." });
    expect(
      await shown.evaluateAll((alerts) =>
        alerts.some((element) => {
          const rect = element.getBoundingClientRect();
          const hit = document.elementFromPoint(
            rect.left + rect.width / 2,
            rect.top + rect.height / 2,
          );
          return hit !== null && element.contains(hit);
        }),
      ),
    ).toBe(true);
  }
  await page.keyboard.press("Escape");
  await expectClosedWithFocusOnGear(page);
});

/** Counts real WebTransport sessions; a remount or reconnect would add one. */
async function countConnections(page: Page) {
  await page.addInitScript(() => {
    const NativeTransport = WebTransport;
    let count = 0;
    window.WebTransport = class extends NativeTransport {
      constructor(url: string | URL, options?: WebTransportOptions) {
        super(url, options);
        count += 1;
        document.documentElement.dataset.connections = String(count);
      }
    };
  });
}

const sessionDump = (page: Page) =>
  page.evaluate(() =>
    Object.fromEntries(
      Object.keys(sessionStorage).map((key) => [key, sessionStorage.getItem(key)]),
    ),
  );

const unblocked = async (page: Page, target: Locator) => {
  await target.scrollIntoViewIfNeeded();
  expect(
    await target.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
      return hit !== null && element.contains(hit);
    }),
  ).toBe(true);
  await page.evaluate(() => window.scrollTo(0, 0));
};

test("native match keeps draft, connection, identity and rack through settings and preferences", async ({
  page,
  browser,
}, testInfo) => {
  test.setTimeout(75_000);
  const phones = await browser.newContext({ viewport: mobile, reducedMotion: "reduce" });
  const first = await phones.newPage();
  const second = await phones.newPage();
  const boardState = await observeSnapshots(page, "1", true);
  const firstState = await observeSnapshots(first, "2");
  const secondState = await observeSnapshots(second, "3");
  for (const client of [page, first, second]) {
    await countConnections(client);
  }
  try {
    await page.goto(`./?${creationQuery}`);
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const href = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    if (href === null) {
      throw new Error("Missing invite");
    }
    for (const [phone, name] of [
      [first, "Ada"],
      [second, "Lin"],
    ] as const) {
      await phone.goto(href);
      await expect(phone.getByLabel("Player name", { exact: true })).toBeVisible();
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => firstState()?.own_rack.tiles.length).toBe(7);
    await expect.poll(() => secondState()?.own_rack.tiles.length).toBe(7);
    const phase = firstState()?.public.phase;
    if (phase?.kind !== "playing") {
      throw new Error("Match did not start");
    }
    const [active, activeState, playerId] =
      phase.active_player === "2"
        ? ([first, firstState, "2"] as const)
        : ([second, secondState, "3"] as const);
    const editor = active.getByRole("region", { name: "Move editor", exact: true });
    const rack = active.getByRole("list", { name: "Your rack", exact: true });
    for (const column of [8, 9]) {
      await rack
        .getByRole("button", { name: /^[A-Z], \d+ points$/ })
        .first()
        .click();
      await editor.getByRole("button", { name: new RegExp(`^Row 8, column ${column}:`) }).click();
    }
    await expect(editor.locator(".tentative-square")).toHaveCount(2);
    await expect(page.locator(".tentative-tile")).toHaveCount(2);

    const before = activeState();
    const boardBefore = boardState();
    const sessionBefore = await sessionDump(active);
    const boardUrl = new URL(page.url());
    const connected = active.getByText(`Connected · Player ${playerId}`, { exact: true });
    await expect(connected).toBeVisible();

    // Player controller (mobile) and shared board (desktop) each exercise the full chrome.
    for (const [client, viewport, label] of [
      [active, mobile, "player-mobile"],
      [page, desktop, "board-desktop"],
    ] as const) {
      await accessible(client);
      await expectChrome(client, viewport);
      await attach(client, testInfo, `${label}-closed.png`);
      let opened = await openSettings(client, english, "keyboard");
      await expectPresentationOnly(opened);
      await expectRepeatedControlsInSync(client, opened);
      await accessible(client);
      await attach(client, testInfo, `${label}-open.png`);
      await client.keyboard.press("Escape");
      await expectClosedWithFocusOnGear(client);
      opened = await openSettings(client);
      await opened.getByRole("button", { name: "Close", exact: true }).click();
      await expectClosedWithFocusOnGear(client);

      await language(client).selectOption("de");
      await expect(client.locator("html")).toHaveAttribute("lang", "de");
      await expect(gear(client, german)).toBeVisible();
      opened = await openSettings(client, german);
      await client.keyboard.press("Escape");
      await expectClosedWithFocusOnGear(client, german);
      await language(client, german).selectOption("en");
      await expect(client.locator("html")).toHaveAttribute("lang", "en");
      await appearance(client).selectOption("dark");
      await expect(client.locator("html")).toHaveClass("dark");
      await attach(client, testInfo, `${label}-dark.png`);
      await appearance(client).selectOption("light");
      await expect(client.locator("html")).not.toHaveClass("dark");
      expect(await client.evaluate(() => document.documentElement.dataset.connections)).toBe("1");
    }

    // Nothing above reconnected, remounted, or altered authority, identity, rack or draft.
    await expect(editor.locator(".tentative-square")).toHaveCount(2);
    await expect(page.locator(".tentative-tile")).toHaveCount(2);
    await expect(connected).toBeVisible();
    expect(await sessionDump(active)).toEqual(sessionBefore);
    expect(activeState()?.own_rack).toEqual(before?.own_rack);
    expect(activeState()?.public.phase).toEqual(before?.public.phase);
    expect(activeState()?.public.ruleset).toEqual(before?.public.ruleset);
    expect(activeState()?.public.dictionary).toEqual(before?.public.dictionary);
    expect(boardState()?.public.board).toEqual(boardBefore?.public.board);
    expect(boardState()?.own_rack.tiles).toEqual([]);
    const boardAfter = new URL(page.url());
    for (const key of ["server", "api", "certificate", "match", "view"]) {
      expect(boardAfter.searchParams.getAll(key)).toEqual(boardUrl.searchParams.getAll(key));
    }
    expect(boardAfter.searchParams.get("lang")).toBe("en");
    for (const client of [page, active]) {
      expect(await client.evaluate(() => document.documentElement.dataset.connections)).toBe("1");
    }
    // The chrome must not obstruct the player's gameplay targets.
    const commit = editor.getByRole("button", { name: "Commit move", exact: true });
    await expect(commit).toBeEnabled();
    await unblocked(active, commit);
    await unblocked(active, rack.getByRole("button").first());
    await unblocked(active, editor.getByRole("button", { name: /^Row 8, column 8:/ }));
  } finally {
    await phones.close();
  }
});
