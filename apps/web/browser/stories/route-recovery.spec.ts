import { expect, test, type Page } from "../fixtures";
import { fixtureQuery } from "../game-fixture";
import { storyTests } from "./story-test";

const absentShare = `s_${"0".repeat(32)}`;
const unknownBoard = `b_${"0".repeat(16)}_${"a".repeat(32)}`;
const recovery = (page: Page) => page.getByRole("link", { name: "Create a new game", exact: true });
const create = (page: Page) => page.getByRole("button", { name: "Create game", exact: true });
const cases = [
  {
    name: "unknown board",
    params: { match: unknownBoard },
    error: "Unable to recover this board session.",
  },
  {
    name: "malformed board",
    params: { match: "../bad" },
    error: "Invalid game connection settings.",
  },
  {
    name: "invalid invite",
    params: { view: "player", match: "../bad" },
    error: "invalid game identifier",
  },
  { name: "missing invite", params: { view: "player" }, error: "invalid game identifier" },
  { name: "unknown view", params: { view: "bogus" }, error: "Unrecognized game link." },
  {
    name: "wrong-case view",
    params: { view: "Player", match: unknownBoard },
    error: "Unrecognized game link.",
  },
  {
    name: "board without launch",
    params: { view: "board" },
    error: "requires a valid launch session",
  },
  {
    name: "malformed layout",
    params: { view: "layout", share: "../bad" },
    error: "Invalid shared board layout link.",
  },
  {
    name: "malformed word list",
    params: { view: "word-list", share: "../bad" },
    error: "Invalid shared word-list link.",
  },
  {
    name: "unavailable layout",
    params: { view: "layout", share: absentShare },
    error: "This shared board layout is not available.",
  },
  {
    name: "unavailable word list",
    params: { view: "word-list", share: absentShare },
    error: "This shared word list is no longer available.",
  },
] satisfies { name: string; params: Record<string, string>; error: string }[];

type World = {
  page: Page;
  original: string;
  matches: number;
  admissions: number;
  shareReads: number;
};
for (const scenario of cases) {
  test.describe(scenario.name, () => {
    async function error(world: World) {
      await expect(world.page.getByRole("alert").first()).toContainText(scenario.error);
      await expect(recovery(world.page)).toBeVisible();
      await expect(create(world.page)).toHaveCount(0);
      await expect(world.page.getByRole("button", { name: "Reconnect", exact: true })).toHaveCount(
        0,
      );
      await expect(world.page.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
      expect(new URL(world.page.url()).search).toBe(world.original);
      expect(world.matches).toBe(0);
      expect(world.admissions).toBe(0);
      if (scenario.name.startsWith("malformed ")) {
        expect(world.shareReads).toBe(0);
      }
    }
    async function setup(world: World) {
      await expect(create(world.page)).toBeEnabled();
      const query = new URL(world.page.url()).searchParams;
      for (const key of ["match", "view", "share", "launch", "session"]) {
        expect(query.has(key)).toBe(false);
      }
      expect(query.get("unrelated")).toBe("kept");
      expect(world.matches).toBe(0);
    }
    async function board(world: World) {
      await expect(
        world.page.getByRole("link", { name: "Join this game", exact: true }),
      ).toBeVisible();
      await expect(
        world.page.getByRole("button", { name: "Start game", exact: true }),
      ).toBeVisible();
      expect(new URL(world.page.url()).searchParams.get("match")).toMatch(/^b_/);
      expect(new URL(world.page.url()).searchParams.get("match")).not.toBe(unknownBoard);
      expect(world.matches).toBe(1);
    }
    storyTests<World>("ROUTE-RECOVERY", {
      open: async ({ page }) => {
        const world: World = { page, original: "", matches: 0, admissions: 0, shareReads: 0 };
        page.on("request", (request) => {
          const path = new URL(request.url()).pathname;
          if (request.method() === "POST" && path.endsWith("/games")) {
            world.matches++;
          }
          if (path.includes("/shared-layouts/") || path.includes("/shared-word-lists/")) {
            world.shareReads++;
          }
          if (request.resourceType() === "webtransport" || path.includes("/matches/")) {
            world.admissions++;
          }
        });
        const query = fixtureQuery();
        for (const [key, value] of Object.entries(scenario.params)) {
          query.set(key, value);
        }
        query.set("unrelated", "kept");
        await page.goto(`./?${query}`);
        world.original = new URL(page.url()).search;
        return world;
      },
      initial: error,
      events: {
        RELOAD_ERROR: async ({ page }) => {
          await page.reload();
        },
        RECOVER: async ({ page }) => {
          await recovery(page).click();
        },
        RELOAD_CREATE: async ({ page }) => {
          await page.reload();
        },
        BACK: async ({ page }) => {
          await page.goBack();
        },
        FORWARD: async ({ page }) => {
          await page.goForward();
        },
        CREATE: async ({ page }) => {
          await create(page).click();
        },
        RELOAD_BOARD: async ({ page }) => {
          await page.reload();
        },
      },
      states: {
        error,
        reloadedError: error,
        create: setup,
        reloadedCreate: setup,
        errorAgain: error,
        createAgain: setup,
        board,
        recoveredBoard: board,
      },
    });
  });
}
