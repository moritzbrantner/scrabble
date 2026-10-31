import { expect, type Page } from "../fixtures";
import { fixtureQuery, gameFixture } from "../game-fixture";
import { readInvite } from "./match-table";
import { storyTests } from "./story-test";

const GERMAN_INVITE = "Diesem Spiel beitreten";
const STORAGE = "scrabble:preferences:v1";
const select = (page: Page, label: string) =>
  page.getByRole("combobox", { name: label, exact: true });
const open = (page: Page, ...lang: readonly string[]) => {
  const query = fixtureQuery();
  for (const value of lang) {
    query.append("lang", value);
  }
  return page.goto(`./?${query.toString()}`);
};
const savedLocale = (page: Page) =>
  page.evaluate(
    (key) =>
      (JSON.parse(localStorage.getItem(key) ?? "null") as { locale?: string } | null)?.locale,
    STORAGE,
  );

storyTests<Page>("ROUTE-QUERY", {
  open: async ({ page }) => {
    await open(page);
    return page;
  },
  initial: async (page) => {
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
  },
  events: {
    OPEN_PLAIN: async (page) => void (await open(page)),
    OPEN_LANG_DE: async (page) => void (await open(page, "de")),
    OPEN_LANG_UNSUPPORTED: async (page) => void (await open(page, "fr-not-offered")),
    OPEN_LANG_AMBIGUOUS: async (page) => void (await open(page, "de", "es")),
    SELECT_DE: async (page) => {
      await select(page, "Interface language").selectOption("de");
      await expect.poll(() => new URL(page.url()).searchParams.getAll("lang")).toEqual(["de"]);
      expect(await savedLocale(page)).toBe("de");
    },
    SELECT_EN: async (page) => {
      await select(page, "Sprache der Oberfläche").selectOption("en");
      await expect.poll(() => new URL(page.url()).searchParams.getAll("lang")).toEqual(["en"]);
      expect(await savedLocale(page)).toBe("en");
    },
    RELOAD: async (page) => {
      const before = page.url();
      await page.reload();
      expect(page.url()).toBe(before);
    },
  },
  states: {
    english: async (page) => {
      await expect(page.locator("html")).toHaveAttribute("lang", "en");
      await expect(select(page, "Interface language")).toHaveValue("en");
      await expect(page.getByRole("button", { name: "Create game", exact: true })).toBeVisible();
      // settings-dialog.md: language changes rewrite only `lang`.
      expect(new URL(page.url()).searchParams.get("api")).toBe(gameFixture.api);
    },
    german: async (page) => {
      await expect(page.locator("html")).toHaveAttribute("lang", "de");
      await expect(select(page, "Sprache der Oberfläche")).toHaveValue("de");
      await expect(
        page.getByRole("button", { name: "Spiel erstellen", exact: true }),
      ).toBeVisible();
      expect(new URL(page.url()).searchParams.get("api")).toBe(gameFixture.api);
    },
  },
});

storyTests<{ page: Page; invite: string }>("ROUTE-INVITE", {
  open: async ({ page }) => {
    const query = fixtureQuery();
    query.set("lang", "de");
    // player-invites.md: invites rebuild the URL and never forward arbitrary fields.
    query.set("stray", "a&b=c d");
    await page.goto(`./?${query.toString()}`);
    return { page, invite: "" };
  },
  initial: async ({ page }) => {
    await expect(page.getByRole("button", { name: "Spiel erstellen", exact: true })).toBeVisible();
    expect(new URL(page.url()).searchParams.get("stray")).toBe("a&b=c d");
  },
  events: {
    CREATE: async (world) => {
      await world.page.getByRole("button", { name: "Spiel erstellen", exact: true }).click();
      world.invite = await readInvite(world.page, GERMAN_INVITE);
    },
    OPEN_LINK: async ({ page, invite }) => {
      await page.goto(invite);
    },
    RELOAD: async ({ page }) => void (await page.reload()),
    BACK: async ({ page }) => void (await page.goBack()),
  },
  states: {
    germanCreate: async () => {},
    germanBoard: async ({ page, invite }) => {
      const url = new URL(invite);
      expect([...url.searchParams.keys()].sort()).toEqual(
        ["api", "certificate", "lang", "match", "server", "view"].sort(),
      );
      // URL-significant characters in the values survive encoding exactly.
      expect(url.searchParams.get("server")).toBe(gameFixture.endpoint);
      expect(url.searchParams.get("api")).toBe(gameFixture.api);
      expect(url.searchParams.get("lang")).toBe("de");
      expect(url.searchParams.get("view")).toBe("player");
      expect(url.searchParams.get("match")).toBe(new URL(page.url()).searchParams.get("match"));
      expect(url.hash).toBe("");
    },
    germanJoin: async ({ page }) => {
      await expect(page.locator("html")).toHaveAttribute("lang", "de");
      await expect(page.getByLabel("Spielername", { exact: true })).toBeVisible();
    },
    germanJoinReloaded: async ({ page, invite }) => {
      expect(page.url()).toBe(invite);
      await expect(page.getByLabel("Spielername", { exact: true })).toBeVisible();
    },
    germanBoardAgain: async ({ page }) => {
      await expect(page.getByRole("link", { name: GERMAN_INVITE, exact: true })).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("lang", "de");
      await expect(page.getByRole("button", { name: "Spiel erstellen", exact: true })).toHaveCount(
        0,
      );
    },
  },
});
