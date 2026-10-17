// Independent acceptance contract for #97, guest-owned slice: import, save,
// reuse, share, copy, withdraw and manage custom word lists in the REAL browser
// app against the real native server fixture (Decisions 1.1, 1.8, 1.16, 1.18,
// 1.20, 1.25, 1.45). Written before implementation by a separate author.
// Server-side validation, bounds, ownership, immutability, restart recovery and
// native replay are proven in crates/scrabble-server/tests/custom_word_lists.rs,
// which also documents the HTTP seams this UI talks to. Account linking is
// deferred by Decision 1.45 to #132 and not asserted here.
//
// UI seams (English copy; existing labels reused where they exist):
// - The create-match page has a region "Custom word lists" with a button
//   "New word list", a list "Saved word lists" whose items show the list name
//   and its distinct word count ("<n> words") and offer buttons "Edit <name>",
//   "Share <name>", "Stop sharing <name>" (only while shared) and
//   "Delete <name>", and a role="note" honest notice (Decision 1.45) saying the
//   lists are kept by this browser and cannot be recovered if it is cleared.
// - Region "Word list editor": a native file input labelled "Word-list file"
//   (choosing a text file fills the multiline textbox "Words" with its text),
//   textbox "Word list name", multiline textbox "Words" (one word per line,
//   editable), button "Save word list"; status "Word list saved." after a save.
//   Invalid input shows role="alert" in the editor with precise feedback: the
//   1-based line number ("line N") for a bad line, or the size/word bound for
//   oversize input. A failed save stores nothing (server or browser).
// - Sharing: after "Share <name>" a link "Word list share link" whose URL
//   carries `view=word-list` and `share=s_<32 hex>` (plus the dev-only
//   connection parameters) and nothing else. Opening it shows region
//   "Shared word list" with the name and word count and buttons "Save a copy"
//   and "Use for a new game" only; "Copy saved." confirms a copy. A withdrawn
//   link shows role="alert" saying the list is no longer available/shared.
// - Match setup: saved and opened-shared lists appear as options of the
//   existing "Word list" combobox (option text contains the list name),
//   selected independently of "Tile profile" and "Board layout" (1.25).
//   Choosing a list never changes the interface language.
// - Private storage: each owned original is one localStorage entry
//   `scrabble:word-list-owner:v1:<wordListId>` holding
//   `{"version":1,"wordListId":"w_<32 hex>","ownerCapability":"<64 hex>",...}`.
//   It is never placed in a URL, sessionStorage, the DOM or a share link, and
//   is removed when the owner deletes the list.
import AxeBuilder from "@axe-core/playwright";
import {
  expect,
  test,
  type APIRequestContext,
  type Browser,
  type Locator,
  type Page,
  type TestInfo,
} from "@playwright/test";
import { z } from "zod";

import { type PlayerSnapshot } from "../src/public-state";
import { gameFixture } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

const query = new URLSearchParams({
  server: gameFixture.endpoint,
  api: gameFixture.api,
  certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
});
const ownerKey = /^scrabble:word-list-owner:v1:(w_[0-9a-f]{32})$/;
const ownerEntry = z.looseObject({
  version: z.literal(1),
  wordListId: z.string().regex(/^w_[0-9a-f]{32}$/),
  ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
});
type Owned = z.infer<typeof ownerEntry>;

/** Every ordered A-Z pair: any two-tile lettered opening forms a valid word. */
const pairs = Array.from({ length: 26 * 26 }, (_, index) =>
  String.fromCharCode(65 + Math.floor(index / 26), 65 + (index % 26)),
).join("\n");
/** No two-letter word exists in this list, so every two-tile opening fails. */
const rejecting = "QUIZ";

async function ownedLists(page: Page): Promise<Owned[]> {
  const raw = await page.evaluate(() =>
    Object.keys(localStorage).map((key) => [key, localStorage.getItem(key) ?? ""] as const),
  );
  const owned: Owned[] = [];
  for (const [key, value] of raw) {
    const match = ownerKey.exec(key);
    if (match) {
      const entry = ownerEntry.parse(JSON.parse(value));
      expect(entry.wordListId).toBe(match[1]);
      owned.push(entry);
    }
  }
  return owned;
}
/** Booleans only, so a failing assertion never prints a private capability. */
async function exposesCapability(page: Page, capability: string) {
  return page.evaluate(
    (secret) =>
      location.href.includes(secret) ||
      Object.keys(sessionStorage).some((key) =>
        (sessionStorage.getItem(key) ?? "").includes(secret),
      ) ||
      document.documentElement.outerHTML.includes(secret),
    capability,
  );
}
async function tabTo(page: Page, target: Locator) {
  for (let count = 0; count < 160; count += 1) {
    if (await target.evaluate((element) => element === document.activeElement)) {
      return;
    }
    await page.keyboard.press("Tab");
  }
  throw new Error("Control is not keyboard reachable");
}
async function attach(testInfo: TestInfo, name: string, page: Page) {
  await testInfo.attach(name, {
    body: await page.screenshot({ fullPage: true }),
    contentType: "image/png",
  });
}
async function noHorizontalOverflow(page: Page) {
  const width = page.viewportSize()?.width ?? 0;
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
    width,
  );
}

const libraryRegion = (page: Page) =>
  page.getByRole("region", { name: "Custom word lists", exact: true });
const editorRegion = (page: Page) =>
  page.getByRole("region", { name: "Word list editor", exact: true });
const savedList = (page: Page) =>
  libraryRegion(page).getByRole("list", { name: "Saved word lists", exact: true });
const savedItem = (page: Page, name: string) =>
  savedList(page).getByRole("listitem").filter({ hasText: name });
const fileInput = (page: Page) => editorRegion(page).getByLabel("Word-list file", { exact: true });
const nameBox = (page: Page) =>
  editorRegion(page).getByRole("textbox", { name: "Word list name", exact: true });
const wordsBox = (page: Page) =>
  editorRegion(page).getByRole("textbox", { name: "Words", exact: true });
const saveButton = (page: Page) =>
  editorRegion(page).getByRole("button", { name: "Save word list", exact: true });
const savedStatus = (page: Page) =>
  page.getByRole("status").filter({ hasText: "Word list saved." });
const wordSelect = (page: Page) => page.getByRole("combobox", { name: "Word list", exact: true });
const tileSelect = (page: Page) =>
  page.getByRole("combobox", { name: "Tile profile", exact: true });
const boardSelect = (page: Page) =>
  page.getByRole("combobox", { name: "Board layout", exact: true });
const sharedRegion = (page: Page) =>
  page.getByRole("region", { name: "Shared word list", exact: true });
const textFile = (name: string, content: string | Buffer) => ({
  name,
  mimeType: "text/plain",
  buffer: typeof content === "string" ? Buffer.from(content, "utf8") : content,
});

async function openLibrary(page: Page) {
  await page.goto(`./?${query}`);
  await expect(libraryRegion(page), "#97 custom word-list library on the create page").toBeVisible({
    timeout: 10_000,
  });
}
async function newList(page: Page) {
  await libraryRegion(page).getByRole("button", { name: "New word list", exact: true }).click();
  await expect(editorRegion(page)).toBeVisible();
}
/** Pointer/touch import of a file through the real file input. */
async function importAndSave(page: Page, name: string, content: string) {
  await newList(page);
  await fileInput(page).setInputFiles(textFile(`${name}.txt`, content));
  await expect(wordsBox(page)).toHaveValue(new RegExp(`^${content.split("\n")[0]}$`, "m"));
  await nameBox(page).fill(name);
  await saveButton(page).click();
  await expect(savedStatus(page)).toBeVisible();
  await expect(savedItem(page, name)).toHaveCount(1);
}
async function optionValue(page: Page, name: string) {
  const option = wordSelect(page).getByRole("option").filter({ hasText: name });
  await expect(option).toHaveCount(1);
  return (await option.getAttribute("value")) ?? "";
}
async function limits(request: APIRequestContext) {
  const response = await request.get(`${gameFixture.api}/word-lists/limits`);
  expect(response.status(), "the server publishes word-list upload bounds").toBe(200);
  return z
    .object({ version: z.literal(1), maxBytes: z.int().positive(), maxWords: z.int().positive() })
    .parse(await response.json());
}
async function ownerRead(request: APIRequestContext, owned: Owned) {
  const response = await request.post(`${gameFixture.api}/word-lists/${owned.wordListId}/read`, {
    data: { version: 1, ownerCapability: owned.ownerCapability },
  });
  return { status: response.status(), body: (await response.json()) as Record<string, unknown> };
}
const wordSet = (text: unknown) =>
  new Set(
    String(text)
      .split(/\r?\n/)
      .map((word) => word.trim())
      .filter(Boolean),
  );

/** Two independent phones join the host's match; the host starts it. */
async function startMatch(browser: Browser, host: Page) {
  const href = await host
    .getByRole("link", { name: "Join this game", exact: true })
    .getAttribute("href");
  expect(href).not.toBeNull();
  const contexts = await Promise.all([
    browser.newContext(),
    browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true }),
  ]);
  const pages = await Promise.all(contexts.map((context) => context.newPage()));
  const readers = await Promise.all(
    pages.map((page, index) => observeSnapshots(page, String(index + 2))),
  );
  for (const [index, page] of pages.entries()) {
    await page.goto(href!);
    await page.getByLabel("Player name", { exact: true }).fill(index === 0 ? "Ada" : "Lin");
    await page.getByRole("button", { name: "Join game", exact: true }).click();
  }
  await host.getByRole("button", { name: "Start game", exact: true }).click();
  for (const reader of readers) {
    await expect.poll(() => reader()?.own_rack.tiles.length).toBeGreaterThan(1);
  }
  return { contexts, pages, readers };
}
type Group = Awaited<ReturnType<typeof startMatch>>;
/** The active phone places its first two lettered tiles across the centre and commits. */
async function playOpening(group: Group, board: () => PlayerSnapshot | undefined) {
  const state = board();
  if (state?.public.phase.kind !== "playing") {
    throw new Error("match is not playing");
  }
  const seat = Number(state.public.phase.active_player) - 2;
  const phone = group.pages[seat]!;
  const own = group.readers[seat]!()!;
  const centre = Math.floor(state.public.configuration.board_size / 2) + 1;
  const letters = own.own_rack.tiles.filter((tile) => tile.face.kind === "letter").slice(0, 2);
  expect(letters).toHaveLength(2);
  const editor = phone.getByRole("region", { name: "Move editor", exact: true });
  for (const [offset, tile] of letters.entries()) {
    await phone
      .getByRole("list", { name: "Your rack", exact: true })
      .locator(`[data-tile-id="${tile.id}"]`)
      .getByRole("button")
      .click();
    await editor
      .getByRole("button", { name: new RegExp(`^Row ${centre}, column ${centre + offset}:`) })
      .click();
  }
  await editor.getByRole("button", { name: "Commit move", exact: true }).click();
  return { phone, editor };
}
async function closeAll(groups: Group[]) {
  for (const group of groups) {
    for (const context of group.contexts) {
      await context.close();
    }
  }
}

test("a guest imports with precise feedback, saves and reuses a word list across sessions using the keyboard", async ({
  page,
  request,
}, testInfo) => {
  test.setTimeout(120_000);
  const name = `Keyboard pairs ${testInfo.workerIndex}-${Date.now()}`;
  await openLibrary(page);
  const bounds = await limits(request);
  const initialTiles = await tileSelect(page).inputValue();
  const initialBoard = await boardSelect(page).inputValue();
  const newButton = libraryRegion(page).getByRole("button", { name: "New word list", exact: true });
  await tabTo(page, newButton);
  await page.keyboard.press("Enter");
  await expect(editorRegion(page)).toBeVisible();

  /** Opens the native file chooser from the keyboard and selects a file. */
  const chooseWithKeyboard = async (file: ReturnType<typeof textFile>) => {
    await tabTo(page, fileInput(page));
    const chooser = page.waitForEvent("filechooser");
    await page.keyboard.press("Space");
    await (await chooser).setFiles(file);
  };
  const saveWithKeyboard = async () => {
    await tabTo(page, saveButton(page));
    await page.keyboard.press("Enter");
  };
  const expectNothingSaved = async () => {
    await expect(savedStatus(page)).toHaveCount(0);
    await expect(savedList(page).getByRole("listitem")).toHaveCount(0);
    expect(await ownedLists(page)).toEqual([]);
  };
  await nameBox(page).fill(name);

  // A bad line is reported by number; nothing is saved.
  await chooseWithKeyboard(textFile("bad-line.txt", "CAT\nD0G\nTA\n"));
  if (await saveButton(page).isEnabled()) {
    await saveWithKeyboard();
  }
  const alert = editorRegion(page).getByRole("alert");
  await expect(alert).toBeVisible();
  await expect(alert).toContainText(/\bline 2\b/i);
  await attach(testInfo, "word-list-upload-error", page);
  await expectNothingSaved();

  // Undecodable bytes, an empty file and an oversize file are rejected too.
  for (const [file, feedback] of [
    [
      textFile("binary.txt", Buffer.from([0x43, 0x41, 0x54, 0x0a, 0xff, 0xfe, 0x0a])),
      /UTF-8|\bline 2\b/i,
    ],
    [textFile("empty.txt", ""), /\S/],
    [
      textFile("oversize.txt", Buffer.alloc(bounds.maxBytes + 1, "AB\n")),
      /too large|limit|at most|maximum/i,
    ],
  ] as const) {
    await chooseWithKeyboard(file);
    if (await saveButton(page).isEnabled()) {
      await saveWithKeyboard();
    }
    await expect(editorRegion(page).getByRole("alert")).toContainText(feedback);
    await expectNothingSaved();
  }

  // A valid file is imported, reviewed, named and saved from the keyboard.
  await chooseWithKeyboard(textFile("pairs.txt", pairs));
  await expect(wordsBox(page)).toHaveValue(/^AA$/m);
  await expect(wordsBox(page)).toHaveValue(/^ZZ$/m);
  await saveWithKeyboard();
  await expect(savedStatus(page)).toBeVisible();
  await expect(savedItem(page, name)).toHaveCount(1);
  await expect(savedItem(page, name)).toContainText(/\b676 words\b/);
  const notice = libraryRegion(page).getByRole("note");
  await expect(notice).toContainText(/this browser/i);
  await expect(notice).toContainText(/recover/i);
  const owned = await ownedLists(page);
  expect(owned).toHaveLength(1);
  expect(await exposesCapability(page, owned[0]!.ownerCapability)).toBe(false);
  await attach(testInfo, "word-list-library", page);

  // Sharing is keyboard operable too.
  await tabTo(page, savedList(page).getByRole("button", { name: `Share ${name}`, exact: true }));
  await page.keyboard.press("Enter");
  await expect(
    libraryRegion(page).getByRole("link", { name: "Word list share link", exact: true }),
  ).toBeVisible();
  await attach(testInfo, "word-list-share", page);

  // A later session in the same browser: reload and a fresh tab.
  await page.reload();
  await expect(savedItem(page, name)).toHaveCount(1);
  const later = await page.context().newPage();
  try {
    await openLibrary(later);
    await expect(savedItem(later, name)).toHaveCount(1);
    expect(await ownedLists(later)).toEqual(owned);
    const lang = await later.locator("html").getAttribute("lang");
    await wordSelect(later).selectOption(await optionValue(later, name));
    await expect(wordSelect(later).locator("option:checked")).toContainText(name);
    // Independent selection: the list leaves tiles, board and interface language alone.
    expect(await tileSelect(later).inputValue()).toBe(initialTiles);
    expect(await boardSelect(later).inputValue()).toBe(initialBoard);
    await expect(later.locator("html")).toHaveAttribute("lang", lang ?? "en");
    expect((await new AxeBuilder({ page: later }).analyze()).violations).toEqual([]);
  } finally {
    await later.close();
  }
});

test("a recipient in an independent browser uses, copies and edits a shared list; withdrawal and edits never alter started matches", async ({
  page,
  browser,
}, testInfo) => {
  test.setTimeout(300_000);
  page.on("dialog", (dialog) => void dialog.accept());
  const name = `Shared pairs ${testInfo.workerIndex}-${Date.now()}`;
  const boardState = await observeSnapshots(page, "1", true);
  const recipientContext = await browser.newContext({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
  });
  const groups: Group[] = [];
  try {
    // Owner: import, save and share.
    await openLibrary(page);
    await importAndSave(page, name, pairs);
    const [owner] = await ownedLists(page);
    expect(owner).toBeDefined();
    await savedList(page)
      .getByRole("button", { name: `Share ${name}`, exact: true })
      .click();
    const shareLink = libraryRegion(page).getByRole("link", {
      name: "Word list share link",
      exact: true,
    });
    await expect(shareLink).toBeVisible();
    const href = new URL((await shareLink.getAttribute("href")) ?? "", page.url());
    expect(href.href.includes(owner!.ownerCapability), "no owner capability in the link").toBe(
      false,
    );
    expect(href.href.includes(owner!.wordListId), "the link is not the private id").toBe(false);
    expect(href.searchParams.get("view")).toBe("word-list");
    const share = href.searchParams.get("share") ?? "";
    expect(share).toMatch(/^s_[0-9a-f]{32}$/);
    expect(
      [...href.searchParams.keys()].filter(
        (key) => !["api", "certificate", "server", "share", "view"].includes(key),
      ),
    ).toEqual([]);
    expect(href.hash).toBe("");

    // Recipient (independent browser storage, phone-sized) opens the link.
    const viewer = await recipientContext.newPage();
    const viewerBoard = await observeSnapshots(viewer, "1", true);
    await viewer.goto(href.href);
    const shared = sharedRegion(viewer);
    await expect(shared).toContainText(name);
    await expect(shared).toContainText(/\b676 words\b/);
    await expect(
      viewer.getByRole("button", { name: /^(Edit|Delete|Share|Stop sharing)\b|^Save word list$/ }),
    ).toHaveCount(0);
    expect(await ownedLists(viewer)).toEqual([]);
    expect(await exposesCapability(viewer, owner!.ownerCapability)).toBe(false);
    expect(
      await viewer.evaluate(
        (secret) => Object.values(localStorage).some((value) => value.includes(secret)),
        owner!.ownerCapability,
      ),
    ).toBe(false);
    await noHorizontalOverflow(viewer);
    await attach(testInfo, "shared-word-list-recipient", viewer);
    expect((await new AxeBuilder({ page: viewer }).analyze()).violations).toEqual([]);

    // Use the shared original for a real match without copying it (1.16).
    await shared.getByRole("button", { name: "Use for a new game", exact: true }).tap();
    await expect(wordSelect(viewer).locator("option:checked")).toContainText(name);
    await viewer.getByRole("button", { name: "Create game", exact: true }).tap();
    await expect(viewer.getByRole("link", { name: "Join this game", exact: true })).toBeVisible();
    await expect.poll(() => viewerBoard()?.public.dictionary.name).toBeTruthy();

    // Save an independent copy, then edit it to reject every two-letter word.
    const copier = await recipientContext.newPage();
    await copier.goto(href.href);
    await sharedRegion(copier).getByRole("button", { name: "Save a copy", exact: true }).tap();
    await expect(copier.getByRole("status").filter({ hasText: "Copy saved." })).toBeVisible();
    const copies = await ownedLists(copier);
    expect(copies).toHaveLength(1);
    expect(copies[0]!.wordListId).not.toBe(owner!.wordListId);
    expect(copies[0]!.ownerCapability).not.toBe(owner!.ownerCapability);
    await openLibrary(copier);
    const copyItem = savedList(copier).getByRole("listitem");
    await expect(copyItem).toHaveCount(1);
    await copyItem.getByRole("button", { name: /^Edit / }).tap();
    await expect(wordsBox(copier)).toHaveValue(/^AA$/m);
    const copyName = `${name} recipient copy`;
    await nameBox(copier).fill(copyName);
    await wordsBox(copier).fill(rejecting);
    await saveButton(copier).tap();
    await expect(savedStatus(copier)).toBeVisible();
    await expect(savedItem(copier, copyName)).toContainText(/\b1 words?\b/);
    const copyRead = await ownerRead(recipientContext.request, copies[0]!);
    expect(copyRead.body.name).toBe(copyName);
    expect(wordSet(copyRead.body.words)).toEqual(wordSet(rejecting));

    // Modified client calls with the public share id or the copy's capability
    // can never manage the original.
    const api = recipientContext.request;
    for (const [path, ownerCapability] of [
      [`/word-lists/${owner!.wordListId}/update`, share],
      [`/word-lists/${owner!.wordListId}/delete`, copies[0]!.ownerCapability],
      [`/word-lists/${owner!.wordListId}/unshare`, copies[0]!.ownerCapability],
      [`/word-lists/${owner!.wordListId}/share`, share],
      [`/shared-word-lists/${share}/update`, share],
      [`/shared-word-lists/${share}/delete`, share],
      [`/shared-word-lists/${share}/unshare`, share],
    ] as const) {
      const response = await api.post(`${gameFixture.api}${path}`, {
        data: { version: 1, ownerCapability, name: "Hijacked", words: "HIJACK" },
      });
      expect([400, 403, 404, 405], `${path} must not manage the original`).toContain(
        response.status(),
      );
    }
    const original = await ownerRead(page.request, owner!);
    expect(original.status).toBe(200);
    expect(original.body.revision).toBe(1);
    expect(original.body.name).toBe(name);
    expect(wordSet(original.body.words)).toEqual(wordSet(pairs));

    // Owner starts a real match with the original and a separately chosen tile profile.
    await page.goto(`./?${query}`);
    await wordSelect(page).selectOption(await optionValue(page, name));
    await tileSelect(page).selectOption({ label: "Classic test tiles" });
    await expect(wordSelect(page).locator("option:checked")).toContainText(name);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const ownerMatch = await startMatch(browser, page);
    groups.push(ownerMatch);
    const pinned = structuredClone(boardState()!.public.dictionary);

    // While it runs, the owner edits, stops sharing and deletes the original.
    const library = await page.context().newPage();
    await openLibrary(library);
    await savedList(library)
      .getByRole("button", { name: `Edit ${name}`, exact: true })
      .click();
    await wordsBox(library).fill(rejecting);
    await saveButton(library).click();
    await expect(savedStatus(library)).toBeVisible();
    await savedList(library)
      .getByRole("button", { name: `Stop sharing ${name}`, exact: true })
      .click();
    await expect(
      libraryRegion(library).getByRole("link", { name: "Word list share link", exact: true }),
    ).toHaveCount(0);
    await savedList(library)
      .getByRole("button", { name: `Delete ${name}`, exact: true })
      .click();
    await expect(savedItem(library, name)).toHaveCount(0);
    await expect(wordSelect(library).getByRole("option").filter({ hasText: name })).toHaveCount(0);
    expect(await ownedLists(library)).toEqual([]);
    await library.close();

    // The started match still validates against its pinned original revision.
    await playOpening(ownerMatch, boardState);
    await expect.poll(() => boardState()?.public.history.length).toBe(1);
    expect(boardState()!.public.dictionary).toEqual(pinned);

    // The withdrawn link now fails visibly and offers nothing.
    await viewer.goto(href.href);
    await expect(viewer.getByRole("alert")).toContainText(
      /no longer (available|shared)|withdrawn|not available/i,
    );
    await expect(viewer.getByRole("button", { name: "Save a copy", exact: true })).toHaveCount(0);
    await expect(
      viewer.getByRole("button", { name: "Use for a new game", exact: true }),
    ).toHaveCount(0);
    await attach(testInfo, "withdrawn-share-recipient", viewer);

    // The recipient's existing copy keeps working, with its OWN validation.
    const host = await recipientContext.newPage();
    const hostBoard = await observeSnapshots(host, "1", true);
    await openLibrary(host);
    await expect(savedList(host).getByRole("listitem")).toHaveCount(1);
    await wordSelect(host).selectOption(await optionValue(host, copyName));
    await host.getByRole("button", { name: "Create game", exact: true }).tap();
    const copyMatch = await startMatch(browser, host);
    groups.push(copyMatch);
    const { editor } = await playOpening(copyMatch, hostBoard);
    await expect(editor.getByRole("status")).toContainText("Not in the dictionary:");
    expect(hostBoard()!.public.history).toEqual([]);
    expect(hostBoard()!.public.dictionary).not.toEqual(pinned);

    // Refreshing the owner's board keeps the pinned match intact.
    const before = structuredClone(boardState()!.public);
    await page.reload();
    await expect(
      page
        .getByRole("table", { name: "Scrabble board" })
        .or(page.getByRole("grid", { name: "Scrabble board" })),
    ).toBeVisible();
    await expect.poll(() => boardState()?.public.history).toEqual(before.history);
    expect(boardState()!.public.dictionary).toEqual(pinned);
    expect(boardState()!.public.players).toEqual(before.players);
  } finally {
    await closeAll(groups);
    await recipientContext.close();
  }
});

test("an incompatible list and tile profile are rejected with an explanation, never silently switched", async ({
  page,
  request,
}, testInfo) => {
  test.setTimeout(90_000);
  const name = `Umlaut list ${testInfo.workerIndex}-${Date.now()}`;
  const hosted = async () =>
    z
      .object({ capacity: z.object({ hostedMatches: z.number() }) })
      .parse(await (await request.get(`${gameFixture.status}/status`)).json()).capacity
      .hostedMatches;
  await openLibrary(page);
  await importAndSave(page, name, "ÖL\nOL");
  const before = await hosted();
  await wordSelect(page).selectOption(await optionValue(page, name));
  await tileSelect(page).selectOption({ label: "Classic test tiles" });
  const selected = [
    await wordSelect(page).inputValue(),
    await tileSelect(page).inputValue(),
    await boardSelect(page).inputValue(),
  ];
  const create = page.getByRole("button", { name: "Create game", exact: true });
  if (await create.isEnabled()) {
    await create.click();
  }
  const alert = page.getByRole("alert").filter({ hasText: /letters missing|Ö/ });
  await expect(alert).toBeVisible();
  await expect(page.getByRole("link", { name: "Join this game", exact: true })).toHaveCount(0);
  expect(await hosted()).toBe(before);
  expect([
    await wordSelect(page).inputValue(),
    await tileSelect(page).inputValue(),
    await boardSelect(page).inputValue(),
  ]).toEqual(selected);
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  await attach(testInfo, "word-list-incompatible", page);
});

test("the library, upload and share UI work by touch on a phone-sized screen", async ({
  browser,
}, testInfo) => {
  test.setTimeout(90_000);
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  const name = `Phone list ${testInfo.workerIndex}-${Date.now()}`;
  try {
    await openLibrary(page);
    await libraryRegion(page).getByRole("button", { name: "New word list", exact: true }).tap();
    await fileInput(page).setInputFiles(textFile("phone-bad.txt", "CAT\nNOT OK\n"));
    await nameBox(page).fill(name);
    if (await saveButton(page).isEnabled()) {
      await saveButton(page).tap();
    }
    await expect(editorRegion(page).getByRole("alert")).toContainText(/\bline 2\b/i);
    await noHorizontalOverflow(page);
    await attach(testInfo, "phone-upload-error", page);
    await fileInput(page).setInputFiles(textFile("phone.txt", "CAT\nTA\nAT\n"));
    await expect(wordsBox(page)).toHaveValue(/^CAT$/m);
    await saveButton(page).tap();
    await expect(savedStatus(page)).toBeVisible();
    await expect(savedItem(page, name)).toContainText(/\b3 words\b/);
    await noHorizontalOverflow(page);
    await attach(testInfo, "phone-library", page);
    await savedList(page)
      .getByRole("button", { name: `Share ${name}`, exact: true })
      .tap();
    const link = libraryRegion(page).getByRole("link", {
      name: "Word list share link",
      exact: true,
    });
    await expect(link).toBeVisible();
    await noHorizontalOverflow(page);
    await attach(testInfo, "phone-share", page);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    const href = new URL((await link.getAttribute("href")) ?? "", page.url());
    const recipient = await browser.newContext({
      viewport: { width: 390, height: 844 },
      hasTouch: true,
    });
    try {
      const viewer = await recipient.newPage();
      await viewer.goto(href.href);
      await expect(sharedRegion(viewer)).toContainText(name);
      await noHorizontalOverflow(viewer);
      await attach(testInfo, "phone-shared-view", viewer);
      expect((await new AxeBuilder({ page: viewer }).analyze()).violations).toEqual([]);
    } finally {
      await recipient.close();
    }
    // Stop sharing by touch removes the link from the library.
    await savedList(page)
      .getByRole("button", { name: `Stop sharing ${name}`, exact: true })
      .tap();
    await expect(link).toHaveCount(0);
  } finally {
    await context.close();
  }
});
