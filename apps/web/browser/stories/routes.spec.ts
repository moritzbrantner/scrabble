import { expect, type APIRequestContext, type Browser, type Page } from "../fixtures";
import { fixtureQuery, gameFixture } from "../game-fixture";
import { directRoutes } from "./stories";
import { storyTests } from "./story-test";

const SESSION = "0123ABCDEFGH";
const hex32 = () => crypto.randomUUID().replaceAll("-", "");
const missingShare = `s_${"0".repeat(32)}`;
const expiredMatch = `g_${"0".repeat(16)}_${"a".repeat(32)}`;
/** A `b_` ID with a current timestamp that this server never derived. */
const unknownMatch = () =>
  `b_${Math.floor(Date.now() / 1000)
    .toString(16)
    .padStart(16, "0")}_${hex32()}`;

const connected = (extra: Record<string, string> = {}) => {
  const query = fixtureQuery();
  for (const [key, value] of Object.entries(extra)) {
    query.append(key, value);
  }
  return `./?${query.toString()}`;
};

async function postJson(request: APIRequestContext, path: string, data: unknown) {
  const response = await request.post(`${gameFixture.api}${path}`, { data });
  expect(response.ok(), `${path} answered ${response.status()}`).toBe(true);
  return (await response.json()) as Record<string, string>;
}
const newMatch = async (request: APIRequestContext) =>
  (
    await postJson(request, "/games", {
      version: 2,
      requestId: hex32(),
      requestedAt: Math.floor(Date.now() / 1000),
    })
  ).matchId ?? "";
async function sharedWordList(request: APIRequestContext) {
  const created = await postJson(request, "/word-lists", {
    version: 1,
    name: "Story words",
    words: "AT\nTA\n",
  });
  const shared = await postJson(request, `/word-lists/${created.wordListId}/share`, {
    version: 1,
    ownerCapability: created.ownerCapability,
  });
  return shared.shareId ?? "";
}

type World = {
  page: Page;
  browser: Browser;
  request: APIRequestContext;
  /** Uncaught exceptions plus console errors that a failed network request did not cause. */
  errors: string[];
  /** Where a tab on this route must be, by exact query. */
  expectedQuery: Map<string, string>;
  route: string;
};

const heading = (page: Page, name: string) => page.getByRole("heading", { name, exact: true });
const create = (page: Page) => page.getByRole("button", { name: "Create game", exact: true });
const previewBoard = (page: Page) => page.getByRole("table", { name: "Scrabble board" });

async function alert(page: Page, text: string | RegExp) {
  await expect(page.getByRole("alert").first()).toContainText(text);
  await expect(create(page)).toHaveCount(0);
}

async function arrive(world: World, route: string) {
  const { page, errors } = world;
  world.route = route;
  const expected = world.expectedQuery.get(route);
  if (expected !== undefined) {
    await expect.poll(() => new URL(page.url()).search).toBe(expected);
  }
  await checks[route]?.(page);
  expect(errors, `page errors on ${route}`).toEqual([]);
}

/** The objective view each route must land on, with no reliance on its URL alone. */
const checks: Record<string, (page: Page) => Promise<void>> = {
  create: async (page) => {
    await expect(create(page)).toBeEnabled();
  },
  board: async (page) => {
    await expect(page.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Start game", exact: true })).toBeVisible();
    await expect(create(page)).toHaveCount(0);
  },
  join: async (page) => {
    await expect(page.getByLabel("Player name", { exact: true })).toBeVisible();
    await expect(page.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
  },
  joined: async (page) => {
    await expect(heading(page, "Ada")).toBeVisible();
    await expect(page.getByText("Waiting for the host to start.", { exact: true })).toBeVisible();
  },
  preview: async (page) => {
    await expect(previewBoard(page)).toBeVisible();
    await expect(
      page.getByText("Multiplayer is not connected yet.", { exact: false }),
    ).toBeVisible();
    await expect(page.getByText("Waiting for players to join.", { exact: true })).toBeVisible();
  },
  previewPlaying: async (page) => {
    await expect(previewBoard(page)).toBeVisible();
    await expect(page.getByRole("status")).toHaveText("Ada's turn");
  },
  // Unknown values fall back to the lobby preview (shared-board.md names only its two fixtures).
  previewBogusFixture: async (page) => checks.preview?.(page),
  unknownView: async (page) => checks.create?.(page),
  connectionInvalid: (page) => alert(page, "Invalid game connection settings."),
  boardWithoutServer: (page) => alert(page, "Invalid game connection settings."),
  boardUnknownMatch: (page) => alert(page, "Unable to recover this board session."),
  boardMalformedMatch: (page) => alert(page, "Invalid game connection settings."),
  inviteNoMatch: (page) => alert(page, "invalid game identifier"),
  inviteMalformed: (page) => alert(page, "invalid game identifier"),
  inviteAmbiguous: (page) => alert(page, "invalid game identifier"),
  inviteExpired: (page) => alert(page, "This game has expired."),
  inviteUnknown: (page) => alert(page, "This game is no longer available."),
  layoutMissing: (page) => alert(page, "This shared board layout is not available."),
  wordListMissing: (page) => alert(page, "This shared word list is no longer available."),
  wordListMalformed: (page) => alert(page, "This shared word list is no longer available."),
  wordListShared: async (page) => {
    await expect(page.getByRole("region", { name: "Shared word list", exact: true })).toBeVisible();
    await expect(heading(page, "Story words")).toBeVisible();
  },
  wordListInUse: async (page) => {
    await expect(create(page)).toBeEnabled();
    await expect(page.getByRole("combobox", { name: "Word list", exact: true })).toContainText(
      "Story words",
    );
  },
  launchInvalid: (page) => alert(page, "Invalid moenarch.com session link."),
  launchUnbound: (page) => alert(page, /session/i),
  launchHosting: async (page) => checks.create?.(page),
};

const urls: Record<(typeof directRoutes)[number], (world: World) => Promise<string> | string> = {
  preview: () => "./",
  previewPlaying: () => "./?fixture=playing",
  previewBogusFixture: () => "./?fixture=bogus",
  unknownView: () => connected({ view: "bogus" }),
  connectionInvalid: () => "./?server=not-a-url&api=not-a-url",
  boardWithoutServer: () => `./?match=${unknownMatch()}`,
  boardUnknownMatch: () => connected({ match: unknownMatch() }),
  boardMalformedMatch: () => connected({ match: "../bad" }),
  inviteNoMatch: () => connected({ view: "player" }),
  join: async ({ request }) => connected({ view: "player", match: await newMatch(request) }),
  inviteMalformed: () => connected({ view: "player", match: "../bad" }),
  inviteAmbiguous: async ({ request }) => {
    const match = await newMatch(request);
    return `${connected({ view: "player", match })}&match=${match}`;
  },
  inviteExpired: () => connected({ view: "player", match: expiredMatch }),
  inviteUnknown: () => connected({ view: "player", match: unknownMatch() }),
  layoutMissing: () => connected({ view: "layout", share: missingShare }),
  wordListMissing: () => connected({ view: "word-list", share: missingShare }),
  wordListMalformed: () => connected({ view: "word-list", share: "../games" }),
  wordListShared: async ({ request }) =>
    connected({ view: "word-list", share: await sharedWordList(request) }),
  launchInvalid: () => connected({ launch: "elsewhere", session: SESSION }),
  launchUnbound: () => connected({ launch: "moenarch", session: SESSION }),
  launchHosting: () => connected({ launch: "moenarch", session: SESSION, view: "board" }),
};

/** Every entry in this tab's history, keyed by route, stays where the URL put it. */
const remember = async (world: World, route: string) => {
  world.expectedQuery.set(route, new URL(world.page.url()).search);
};

storyTests<World>("ROUTE-NAV", {
  open: async ({ page, browser, request }) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(`${error.name}: ${error.message}`));
    page.on("console", (message) => {
      // The browser logs refused requests itself. The guard routes exist to meet these answers.
      if (message.type() === "error" && !message.text().startsWith("Failed to load resource")) {
        errors.push(message.text());
      }
    });
    await page.goto(connected());
    const world: World = {
      page,
      browser,
      request,
      errors,
      expectedQuery: new Map(),
      route: "create",
    };
    await remember(world, "create");
    return world;
  },
  initial: (world) => arrive(world, "create"),
  events: {
    ...Object.fromEntries(
      directRoutes.map((route) => [
        `OPEN_${route}`,
        async (world: World) => {
          await world.page.goto(await urls[route](world));
          await arrive(world, route);
          await remember(world, route);
        },
      ]),
    ),
    CREATE: async (world) => {
      await create(world.page).click();
      await expect(
        world.page.getByRole("link", { name: "Join this game", exact: true }),
      ).toBeVisible();
      expect(new URL(world.page.url()).searchParams.get("match")).toMatch(/^b_/);
      await remember(world, "board");
    },
    FOLLOW_INVITE: async (world) => {
      await world.page.getByRole("link", { name: "Join this game", exact: true }).click();
      await world.page.waitForURL(/view=player/);
      await remember(world, "join");
    },
    JOIN: async (world) => {
      await world.page.getByLabel("Player name", { exact: true }).fill("Ada");
      await world.page.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(heading(world.page, "Ada")).toBeVisible();
      await remember(world, "joined");
    },
    USE_WORD_LIST: async (world) => {
      await world.page.getByRole("button", { name: "Use for a new game", exact: true }).click();
      await expect(create(world.page)).toBeVisible();
      expect(new URL(world.page.url()).searchParams.has("share")).toBe(false);
    },
    RELOAD: async (world) => {
      await world.page.reload();
    },
    BACK: async (world) => {
      await world.page.goBack();
    },
    FORWARD: async (world) => {
      await world.page.goForward();
    },
  },
  states: Object.fromEntries(
    Object.keys(checks).map((route) => [route, (world: World) => arrive(world, route)]),
  ),
});
