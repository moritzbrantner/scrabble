// Independent acceptance contract for #101, first slice: guest-owned custom
// regular square/hex board layouts in the REAL browser app against the real
// native server fixture (Decisions 1.3, 1.10-1.13, 1.31, 1.33, 1.35, 1.37, 1.45).
// Written before implementation by a separate author. Server-side validation,
// immutability and ownership are proven natively in
// crates/scrabble-server/tests/custom_board_layouts.rs, which also documents
// the HTTP seams this UI talks to.
//
// UI seams (English copy; existing labels reused where they exist):
// - The create-match page has a region "Custom board layouts" with a button
//   "New custom board", a list "Saved board layouts" whose items show the
//   layout name and offer buttons "Edit <name>", "Share <name>" and
//   "Delete <name>", and a role="note" honest notice (Decision 1.45) saying the
//   layouts are kept by this browser and cannot be recovered if it is cleared.
// - Region "Board layout editor": textbox "Layout name"; radios "Square" /
//   "Hexagon"; spinbutton "Board size" (odd storage side, like catalog
//   `boardSize`); radio group "Premium type" with the existing premium copy
//   ("Normal square", "Double letter", "Triple letter", "Double word",
//   "Triple word"); hex-only radio group "Premium application" with
//   /all formed words/i and /main word only/i; a grid "Layout cells" of buttons
//   named "Row R, column C: <premium copy>" carrying 0-based `data-row` and
//   `data-column`, with one roving tabindex=0 cell, arrow-key movement by
//   storage row/column and Enter/Space (or click/tap) painting the selected
//   premium type; button "Save layout" (disabled while the draft is invalid,
//   with an alert explaining why); status "Layout saved." after saving.
// - Sharing: after "Share <name>" a link "Layout share link" whose URL carries
//   `view=layout` and `share=s_<32 hex>` (plus the dev-only connection
//   parameters) and nothing else. Opening it shows region "Shared board layout"
//   with the name, the existing "Board preview", and buttons "Save a copy" and
//   "Use for a new game" only; "Copy saved." confirms a copy.
// - Match setup: saved and opened-shared layouts appear as options of the
//   existing "Board layout" combobox (option text contains the layout name),
//   selected independently of "Word list" and "Tile profile" (Decision 1.31).
// - Private storage: each owned original is one localStorage entry
//   `scrabble:layout-owner:v1:<layoutId>` holding
//   `{"version":1,"layoutId":"l_<32 hex>","ownerCapability":"<64 hex>",...}`.
//   It is never placed in a URL, sessionStorage or a share link, and is
//   removed when the owner deletes the layout.
// - Withdrawal (Decision 1.35, #134): an owner's shared row offers
//   "Stop sharing <name>", confirmed by the dialog "Stop sharing <name>?"; afterwards the old link shows the alert
//   "This shared board layout is not available." while copies keep working.
// Account linking (#132) is successor scope.
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { z } from "zod";

import { gameFixture, selectOpening } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

const query = new URLSearchParams({
  server: gameFixture.endpoint,
  api: gameFixture.api,
  certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
});
const directory = dirname(process.env.SCRABBLE_GAME_FIXTURE!);
const executable = resolve(
  process.env.CARGO_TARGET_DIR ?? "target",
  "debug/examples/browser_game_fixture",
);
const opening = z.strictObject({ requestId: z.string(), requestedAt: z.number() });
const ownerKey = /^scrabble:layout-owner:v1:(l_[0-9a-f]{32})$/;
const ownerEntry = z.looseObject({
  version: z.literal(1),
  layoutId: z.string().regex(/^l_[0-9a-f]{32}$/),
  ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
});

type Owned = z.infer<typeof ownerEntry>;
async function ownedLayouts(page: Page): Promise<Owned[]> {
  const raw = await page.evaluate(() =>
    Object.keys(localStorage).map((key) => [key, localStorage.getItem(key) ?? ""] as const),
  );
  const owned: Owned[] = [];
  for (const [key, value] of raw) {
    const match = ownerKey.exec(key);
    if (match) {
      const entry = ownerEntry.parse(JSON.parse(value));
      expect(entry.layoutId).toBe(match[1]);
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
  for (let count = 0; count < 120; count += 1) {
    if (await target.evaluate((element) => element === document.activeElement)) {
      return;
    }
    await page.keyboard.press("Tab");
  }
  throw new Error("Control is not keyboard reachable");
}
const editorRegion = (page: Page) =>
  page.getByRole("region", { name: "Board layout editor", exact: true });
const editorCells = (page: Page) =>
  editorRegion(page)
    .getByRole("grid", { name: "Layout cells", exact: true })
    .getByRole("button", { name: /^Row \d+, column \d+:/ });
const editorCell = (page: Page, row: number, column: number) =>
  editorRegion(page)
    .getByRole("grid", { name: "Layout cells", exact: true })
    .getByRole("button", { name: new RegExp(`^Row ${row + 1}, column ${column + 1}:`) });

/** Select a premium type and paint one cell using only the keyboard. */
async function paintWithKeyboard(
  page: Page,
  kind: string,
  row: number,
  column: number,
  size: number,
) {
  const editor = editorRegion(page);
  const types = editor.getByRole("radiogroup", { name: "Premium type", exact: true });
  const target = types.getByRole("radio", { name: kind, exact: true });
  await tabTo(page, types.getByRole("radio", { checked: true }));
  for (let step = 0; step < 6 && !(await target.isChecked()); step += 1) {
    await page.keyboard.press("ArrowDown");
  }
  await expect(target).toBeChecked();
  const roving = editor
    .getByRole("grid", { name: "Layout cells", exact: true })
    .locator('[tabindex="0"]');
  await tabTo(page, roving);
  let currentRow = Number(await roving.getAttribute("data-row"));
  let currentColumn = Number(await roving.getAttribute("data-column"));
  const center = Math.floor(size / 2);
  // The centre row and column intersect every on-board row/column, so this
  // route never crosses an off-board hex storage corner.
  for (const [nextRow, nextColumn] of [
    [center, currentColumn],
    [center, center],
    [row, center],
    [row, column],
  ] as const) {
    while (currentRow !== nextRow) {
      await page.keyboard.press(currentRow > nextRow ? "ArrowUp" : "ArrowDown");
      currentRow += currentRow > nextRow ? -1 : 1;
    }
    while (currentColumn !== nextColumn) {
      await page.keyboard.press(currentColumn > nextColumn ? "ArrowLeft" : "ArrowRight");
      currentColumn += currentColumn > nextColumn ? -1 : 1;
    }
  }
  await expect(editorCell(page, row, column)).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(editorCell(page, row, column)).toHaveAccessibleName(
    new RegExp(`^Row ${row + 1}, column ${column + 1}:.*${kind}`),
  );
}
/** Select a premium type and paint one cell with the pointer. */
async function paintWithPointer(page: Page, kind: string, row: number, column: number) {
  await editorRegion(page)
    .getByRole("radiogroup", { name: "Premium type", exact: true })
    .getByRole("radio", { name: kind, exact: true })
    .check();
  await editorCell(page, row, column).click();
  await expect(editorCell(page, row, column)).toHaveAccessibleName(
    new RegExp(`^Row ${row + 1}, column ${column + 1}:.*${kind}`),
  );
}
async function saveLayout(page: Page, keyboard = false) {
  const save = editorRegion(page).getByRole("button", { name: "Save layout", exact: true });
  await expect(save).toBeEnabled();
  if (keyboard) {
    await tabTo(page, save);
    await page.keyboard.press("Enter");
  } else {
    await save.click();
  }
  await expect(page.getByRole("status").filter({ hasText: "Layout saved." })).toBeVisible();
}
const libraryRegion = (page: Page) =>
  page.getByRole("region", { name: "Custom board layouts", exact: true });
const savedList = (page: Page) =>
  libraryRegion(page).getByRole("list", { name: "Saved board layouts", exact: true });
async function boardOptionValue(page: Page, name: string) {
  const option = page
    .getByRole("combobox", { name: "Board layout", exact: true })
    .getByRole("option")
    .filter({ hasText: name });
  await expect(option).toHaveCount(1);
  return (await option.getAttribute("value")) ?? "";
}

async function activate(page: Page, target: Locator, touch: boolean) {
  if (touch) {
    await target.scrollIntoViewIfNeeded();
    await target.tap();
  } else {
    await tabTo(page, target);
    await page.keyboard.press("Enter");
  }
}
/** Same real phone controls as hexagonal-board.spec.ts, on a side-7 hex. */
async function place(
  page: Page,
  cells: readonly (readonly [number, number, string])[],
  touch: boolean,
) {
  const rack = page.getByRole("list", { name: "Your rack", exact: true });
  const editor = page.getByRole("region", { name: "Move editor", exact: true });
  for (const [row, column, face] of cells) {
    await activate(
      page,
      rack.getByRole("button", { name: new RegExp(`^${face}, \\d+ points$`) }).first(),
      touch,
    );
    const target = editor.getByRole("button", {
      name: new RegExp(`^Row ${row + 1}, column ${column + 1}:`),
    });
    if (touch) {
      await target.scrollIntoViewIfNeeded();
      await target.tap();
      continue;
    }
    const selected = editor.locator('[data-square-index][tabindex="0"]');
    await tabTo(page, selected);
    const index = Number(await selected.getAttribute("data-square-index"));
    let currentRow = Math.floor(index / 7);
    let currentColumn = index % 7;
    for (const [nextRow, nextColumn] of [
      [3, currentColumn],
      [3, 3],
      [row, 3],
      [row, column],
    ] as const) {
      while (currentRow !== nextRow) {
        await page.keyboard.press(currentRow > nextRow ? "ArrowUp" : "ArrowDown");
        currentRow += currentRow > nextRow ? -1 : 1;
      }
      while (currentColumn !== nextColumn) {
        await page.keyboard.press(currentColumn > nextColumn ? "ArrowLeft" : "ArrowRight");
        currentColumn += currentColumn > nextColumn ? -1 : 1;
      }
    }
    await expect(target).toBeFocused();
    await page.keyboard.press("Enter");
  }
}

test("a guest designs, edits and plays an authoritative match on a custom hex layout that later library edits never alter", async ({
  page,
  browser,
}, testInfo) => {
  test.setTimeout(180_000);
  page.on("dialog", (dialog) => void dialog.accept());
  const name = `Guest hex ${testInfo.workerIndex}-${Date.now()}`;
  const desktop = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const phone = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true });
  const ada = await desktop.newPage();
  const lin = await phone.newPage();
  const boardState = await observeSnapshots(page, "1", true);
  const adaState = await observeSnapshots(ada, "2");
  const linState = await observeSnapshots(lin, "3");
  try {
    await page.goto(`./?${query}`);
    await expect(libraryRegion(page), "#101 custom board library on the create page").toBeVisible({
      timeout: 10_000,
    });
    await libraryRegion(page)
      .getByRole("button", { name: "New custom board", exact: true })
      .click();
    const editor = editorRegion(page);
    await editor.getByRole("textbox", { name: "Layout name", exact: true }).fill(name);
    await editor.getByRole("radio", { name: "Hexagon", exact: true }).check();
    const size = editor.getByRole("spinbutton", { name: "Board size", exact: true });
    // Resizing keeps a complete regular hex: 3R²+3R+1 cells, no holes.
    await size.fill("9");
    await expect(editorCells(page)).toHaveCount(61);
    await size.fill("8");
    await expect(editor.getByRole("alert")).toBeVisible();
    await expect(editor.getByRole("button", { name: "Save layout", exact: true })).toBeDisabled();
    await size.fill("7");
    await expect(editorCells(page)).toHaveCount(37);
    await expect(editor.getByRole("alert")).toHaveCount(0);
    await editor
      .getByRole("radiogroup", { name: "Premium application", exact: true })
      .getByRole("radio", { name: /main word only/i })
      .check();
    await paintWithKeyboard(page, "Double word", 3, 3, 7);
    await saveLayout(page, true);
    const notice = libraryRegion(page).getByRole("note");
    await expect(notice).toContainText(/this browser/i);
    await expect(notice).toContainText(/recover/i);
    const owned = await ownedLayouts(page);
    expect(owned).toHaveLength(1);
    const [original] = owned;
    expect(await exposesCapability(page, original!.ownerCapability)).toBe(false);

    // Edit the saved original: the next match uses the new version.
    await expect(savedList(page).getByRole("listitem").filter({ hasText: name })).toHaveCount(1);
    await savedList(page)
      .getByRole("button", { name: `Edit ${name}`, exact: true })
      .click();
    await expect(editorCells(page)).toHaveCount(37);
    await expect(editorCell(page, 3, 3)).toHaveAccessibleName(/Double word/);
    await paintWithKeyboard(page, "Triple word", 3, 3, 7);
    await saveLayout(page, true);
    expect(await ownedLayouts(page)).toEqual(owned);

    await page
      .getByRole("combobox", { name: "Word list", exact: true })
      .selectOption("acceptance-hex-words");
    await page
      .getByRole("combobox", { name: "Tile profile", exact: true })
      .selectOption("acceptance-multi-letter-tiles");
    await page
      .getByRole("combobox", { name: "Board layout", exact: true })
      .selectOption(await boardOptionValue(page, name));
    expect(await page.getByRole("combobox", { name: "Word list", exact: true }).inputValue()).toBe(
      "acceptance-hex-words",
    );
    const preview = page
      .getByRole("table", { name: "Board preview", exact: true })
      .or(page.getByRole("grid", { name: "Board preview", exact: true }));
    const previewCells = preview.getByRole("cell").or(preview.getByRole("gridcell"));
    await expect(previewCells).toHaveCount(37);
    await expect(
      preview
        .getByRole("cell", { name: /^Row 4, column 4:.*Triple word/ })
        .or(preview.getByRole("gridcell", { name: /^Row 4, column 4:.*Triple word/ })),
    ).toHaveCount(1);
    const deal = opening.parse(
      JSON.parse(
        execFileSync(executable, ["--hex-opening", directory], {
          encoding: "utf8",
          timeout: 60_000,
        }),
      ),
    );
    await selectOpening(page, deal);
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const href = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    if (!href) {
      throw new Error("Missing actual match invitation");
    }
    for (const [client, player] of [
      [ada, "Ada"],
      [lin, "Lin"],
    ] as const) {
      await client.goto(href);
      await client.getByLabel("Player name", { exact: true }).fill(player);
      await client.getByRole("button", { name: "Join game", exact: true }).click();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
    await expect.poll(() => linState()?.own_rack.tiles.length).toBe(7);
    const configuration = structuredClone(boardState()!.public.configuration);
    const geometry = z
      .object({
        board_shape: z.literal("hexagon"),
        board_size: z.literal(7),
        premiums: z.array(z.string()).length(37),
        premium_policy: z.literal("main_word_only"),
      })
      .parse(configuration);
    // Row-major on-board index 18 is the centre (3,3): the EDITED version.
    expect(geometry.premiums[18]).toBe("triple_word");
    expect(geometry.premiums.filter((premium) => premium !== "normal")).toEqual(["triple_word"]);
    for (const client of [ada, lin]) {
      await expect(
        client
          .getByRole("region", { name: "Move editor", exact: true })
          .getByRole("button", { name: /^Row \d+, column \d+:/ }),
      ).toHaveCount(37);
    }

    // Centre opening along one hex axis, scored with the custom TW (SCH 8 + A 1).
    await place(
      ada,
      [
        [3, 3, "SCH"],
        [3, 4, "A"],
      ],
      false,
    );
    await activate(ada, ada.getByRole("button", { name: "Commit move", exact: true }), false);
    await expect.poll(() => boardState()?.public.history.length).toBe(1);
    expect(boardState()!.public.history[0]!.action).toMatchObject({
      kind: "commit",
      words: ["SCHA"],
      move_score: 27,
    });

    // While the match runs, the owner edits and then deletes the library layout.
    const library = await page.context().newPage();
    await library.goto(`./?${query}`);
    await savedList(library)
      .getByRole("button", { name: `Edit ${name}`, exact: true })
      .click();
    await paintWithPointer(library, "Normal square", 3, 3);
    await saveLayout(library);
    await savedList(library)
      .getByRole("button", { name: `Delete ${name}`, exact: true })
      .click();
    await expect(savedList(library).getByRole("listitem").filter({ hasText: name })).toHaveCount(0);
    await expect(
      library
        .getByRole("combobox", { name: "Board layout", exact: true })
        .getByRole("option")
        .filter({ hasText: name }),
    ).toHaveCount(0);
    expect(await ownedLayouts(library)).toEqual([]);
    await library.close();

    // The started match keeps its immutable version, including crossing words
    // on the other hex axes (Decisions 1.10/1.11) and a board reconnect.
    await place(
      lin,
      [
        [2, 4, "A"],
        [2, 5, "A"],
      ],
      true,
    );
    await activate(lin, lin.getByRole("button", { name: "Commit move", exact: true }), true);
    await expect.poll(() => boardState()?.public.history.length).toBe(2);
    const crossing = boardState()!.public.history[1]!.action;
    expect(crossing).toMatchObject({ kind: "commit", move_score: 15 });
    expect(crossing.kind === "commit" ? crossing.words.length : 0).toBe(4);
    expect(boardState()!.public.configuration).toEqual(configuration);
    const before = structuredClone(boardState()!.public);
    await page.reload();
    await expect(
      page
        .getByRole("table", { name: "Scrabble board" })
        .or(page.getByRole("grid", { name: "Scrabble board" })),
    ).toBeVisible();
    await expect.poll(() => boardState()?.public.history).toEqual(before.history);
    expect(boardState()!.public.configuration).toEqual(configuration);
    expect(boardState()!.public.players).toEqual(before.players);
    for (const client of [page, ada]) {
      expect((await new AxeBuilder({ page: client }).analyze()).violations).toEqual([]);
    }
  } finally {
    await Promise.all([desktop.close(), phone.close()]);
  }
});

test("a recipient in an independent browser uses and copies a shared square layout without gaining edit access", async ({
  page,
  browser,
}, testInfo) => {
  test.setTimeout(150_000);
  page.on("dialog", (dialog) => void dialog.accept());
  const name = `Shared square ${testInfo.workerIndex}-${Date.now()}`;
  const recipientContext = await browser.newContext({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
  });
  try {
    // Owner: pointer-authored resized square board.
    await page.goto(`./?${query}`);
    await expect(libraryRegion(page), "#101 custom board library on the create page").toBeVisible({
      timeout: 10_000,
    });
    await libraryRegion(page)
      .getByRole("button", { name: "New custom board", exact: true })
      .click();
    const editor = editorRegion(page);
    await editor.getByRole("textbox", { name: "Layout name", exact: true }).fill(name);
    await editor.getByRole("radio", { name: "Square", exact: true }).check();
    await editor.getByRole("spinbutton", { name: "Board size", exact: true }).fill("9");
    await expect(editorCells(page)).toHaveCount(81);
    await expect(
      editor.getByRole("radiogroup", { name: "Premium application", exact: true }),
    ).toHaveCount(0);
    await paintWithPointer(page, "Double word", 4, 4);
    await paintWithPointer(page, "Triple letter", 1, 1);
    await saveLayout(page);
    const [owner] = await ownedLayouts(page);
    expect(owner).toBeDefined();
    await savedList(page)
      .getByRole("button", { name: `Share ${name}`, exact: true })
      .click();
    const shareLink = libraryRegion(page).getByRole("link", {
      name: "Layout share link",
      exact: true,
    });
    await expect(shareLink).toBeVisible();
    const href = new URL((await shareLink.getAttribute("href")) ?? "", page.url());
    expect(
      href.href.includes(owner!.ownerCapability),
      "share link carries no owner capability",
    ).toBe(false);
    expect(href.href.includes(owner!.layoutId), "share link is not the private original id").toBe(
      false,
    );
    expect(href.searchParams.get("view")).toBe("layout");
    expect(href.searchParams.get("share")).toMatch(/^s_[0-9a-f]{32}$/);
    expect(
      [...href.searchParams.keys()].filter(
        (key) => !["api", "certificate", "server", "share", "view"].includes(key),
      ),
    ).toEqual([]);
    expect(href.hash).toBe("");

    // Recipient: independent browser storage opens the shared layout.
    const recipient = await recipientContext.newPage();
    const recipientBoard = await observeSnapshots(recipient, "1", true);
    await recipient.goto(href.href);
    const shared = recipient.getByRole("region", { name: "Shared board layout", exact: true });
    await expect(shared).toContainText(name);
    const preview = shared
      .getByRole("table", { name: "Board preview", exact: true })
      .or(shared.getByRole("grid", { name: "Board preview", exact: true }));
    await expect(preview.getByRole("cell").or(preview.getByRole("gridcell"))).toHaveCount(81);
    await expect(preview.getByRole("cell", { name: /^Row 5, column 5:.*Double word/ })).toHaveCount(
      1,
    );
    await expect(
      recipient.getByRole("button", { name: /^(Edit|Delete|Share)\b|^Save layout$/ }),
    ).toHaveCount(0);
    expect(await ownedLayouts(recipient)).toEqual([]);
    expect(await exposesCapability(recipient, owner!.ownerCapability)).toBe(false);
    expect(
      await recipient.evaluate(
        (secret) => Object.values(localStorage).some((value) => value.includes(secret)),
        owner!.ownerCapability,
      ),
    ).toBe(false);

    // Use it for a real match without copying it (Decision 1.33).
    await shared.getByRole("button", { name: "Use for a new game", exact: true }).click();
    const boardLayout = recipient.getByRole("combobox", { name: "Board layout", exact: true });
    await expect(boardLayout.locator("option:checked")).toContainText(name);
    await recipient.getByRole("button", { name: "Create game", exact: true }).click();
    await expect(
      recipient.getByRole("link", { name: "Join this game", exact: true }),
    ).toBeVisible();
    await expect.poll(() => recipientBoard()?.public.configuration.board_size).toBe(9);
    const used = recipientBoard()!.public.configuration;
    expect(used.board_shape ?? "square").toBe("square");
    expect(used.premiums[4 * 9 + 4]).toBe("double_word");
    expect(used.premiums[1 * 9 + 1]).toBe("triple_letter");

    // Save an independent copy, then resize and repaint it.
    const copyPage = await recipientContext.newPage();
    await copyPage.goto(href.href);
    await copyPage
      .getByRole("region", { name: "Shared board layout", exact: true })
      .getByRole("button", { name: "Save a copy", exact: true })
      .click();
    await expect(copyPage.getByRole("status").filter({ hasText: "Copy saved." })).toBeVisible();
    const copies = await ownedLayouts(copyPage);
    expect(copies).toHaveLength(1);
    expect(copies[0]!.layoutId).not.toBe(owner!.layoutId);
    expect(copies[0]!.ownerCapability).not.toBe(owner!.ownerCapability);
    await copyPage.goto(`./?${query}`);
    const copyItem = savedList(copyPage).getByRole("listitem");
    await expect(copyItem).toHaveCount(1);
    await copyItem.getByRole("button", { name: /^Edit / }).click();
    await expect(editorCells(copyPage)).toHaveCount(81);
    await editorRegion(copyPage)
      .getByRole("spinbutton", { name: "Board size", exact: true })
      .fill("11");
    await expect(editorCells(copyPage)).toHaveCount(121);
    await paintWithPointer(copyPage, "Triple word", 5, 5);
    await saveLayout(copyPage);

    // The owner's original is unchanged by the copy edit ...
    await page.goto(`./?${query}`);
    await savedList(page)
      .getByRole("button", { name: `Edit ${name}`, exact: true })
      .click();
    await expect(editorCells(page)).toHaveCount(81);
    await expect(editorCell(page, 4, 4)).toHaveAccessibleName(/Double word/);
    await expect(editorCell(page, 1, 1)).toHaveAccessibleName(/Triple letter/);
    await expect(
      editorRegion(page).getByRole("button", { name: /^Row \d+, column \d+:.*Triple word/ }),
    ).toHaveCount(0);
    // ... and the owner's next edit never reaches the recipient's copy.
    await paintWithPointer(page, "Normal square", 1, 1);
    await saveLayout(page);
    await copyPage.reload();
    await savedList(copyPage)
      .getByRole("listitem")
      .getByRole("button", { name: /^Edit / })
      .click();
    await expect(editorCells(copyPage)).toHaveCount(121);
    await expect(editorCell(copyPage, 5, 5)).toHaveAccessibleName(/Triple word/);
    // The share link shows the owner's current original, not the copy.
    await recipient.goto(href.href);
    const reopened = recipient.getByRole("region", { name: "Shared board layout", exact: true });
    const current = reopened
      .getByRole("table", { name: "Board preview", exact: true })
      .or(reopened.getByRole("grid", { name: "Board preview", exact: true }));
    await expect(current.getByRole("cell").or(current.getByRole("gridcell"))).toHaveCount(81);
    await expect(
      current.getByRole("cell", { name: /^Row 2, column 2:.*Triple letter/ }),
    ).toHaveCount(0);
    // Only the owner withdraws the link; the recipient's copy keeps working.
    await expect(
      recipient.getByRole("button", { name: /^Stop sharing\b/ }),
      "recipients cannot withdraw",
    ).toHaveCount(0);
    await page.goto(`./?${query}`);
    const stopSharing = savedList(page).getByRole("button", {
      name: `Stop sharing ${name}`,
      exact: true,
    });
    // Withdrawal asks first; dismissing keeps the link working.
    page.removeAllListeners("dialog");
    const asked: string[] = [];
    page.once("dialog", (dialog) => {
      asked.push(dialog.message());
      void dialog.dismiss();
    });
    await stopSharing.click();
    await expect.poll(() => asked).toEqual([`Stop sharing ${name}?`]);
    await expect(stopSharing).toBeVisible();
    await recipient.goto(href.href);
    await expect(
      recipient.getByRole("region", { name: "Shared board layout", exact: true }),
    ).toContainText(name);
    page.on("dialog", (dialog) => void dialog.accept());
    await stopSharing.click();
    await expect(
      libraryRegion(page).getByRole("status").filter({ hasText: "Sharing stopped." }),
    ).toBeVisible();
    await expect(libraryRegion(page).getByRole("link", { name: "Layout share link" })).toHaveCount(
      0,
    );
    await page.reload();
    await expect(
      savedList(page).getByRole("button", { name: `Share ${name}`, exact: true }),
    ).toBeVisible();
    await expect(
      savedList(page).getByRole("button", { name: `Stop sharing ${name}`, exact: true }),
    ).toHaveCount(0);
    await recipient.goto(href.href);
    await expect(
      recipient
        .getByRole("alert")
        .filter({ hasText: "This shared board layout is not available." }),
    ).toBeVisible();
    await copyPage.goto(`./?${query}`);
    await expect(savedList(copyPage).getByRole("listitem")).toHaveCount(1);
    for (const client of [page, recipient]) {
      expect((await new AxeBuilder({ page: client }).analyze()).violations).toEqual([]);
    }
  } finally {
    await recipientContext.close();
  }
});

// Retrospective independent acceptance for Decision 1.37's established bounds
// and premium preservation while an unfinished size edit changes geometry.
test("the editor respects shape-specific minimum sizes", async ({ page }) => {
  await page.goto(`./?${query}`);
  await libraryRegion(page).getByRole("button", { name: "New custom board", exact: true }).click();
  const editor = editorRegion(page);
  await editor.getByRole("textbox", { name: "Layout name", exact: true }).fill("Small square");
  const size = editor.getByRole("spinbutton", { name: "Board size", exact: true });
  const save = editor.getByRole("button", { name: "Save layout", exact: true });
  await size.fill("1");
  await expect(editorCells(page)).toHaveCount(1);
  await expect(save).toBeEnabled();
  await editor.getByRole("radio", { name: "Hexagon", exact: true }).check();
  await expect(editor.getByRole("alert")).toBeVisible();
  await expect(save).toBeDisabled();
  await editor.getByRole("radio", { name: "Square", exact: true }).check();
  await expect(editorCells(page)).toHaveCount(1);
  await expect(editor.getByRole("alert")).toHaveCount(0);
  await expect(save).toBeEnabled();
  await saveLayout(page);
});

test("changing shape during an invalid size edit preserves the centered premium", async ({
  page,
}) => {
  await page.goto(`./?${query}`);
  await libraryRegion(page).getByRole("button", { name: "New custom board", exact: true }).click();
  const editor = editorRegion(page);
  await editor.getByRole("textbox", { name: "Layout name", exact: true }).fill("Preserved center");
  const size = editor.getByRole("spinbutton", { name: "Board size", exact: true });
  await size.fill("9");
  await paintWithPointer(page, "Triple word", 4, 4);
  await size.fill("8");
  await editor.getByRole("radio", { name: "Hexagon", exact: true }).check();
  await expect(editor.getByRole("button", { name: "Save layout", exact: true })).toBeDisabled();
  await size.fill("7");
  await expect(editorCells(page)).toHaveCount(37);
  await expect(editorCell(page, 3, 3)).toHaveAccessibleName(/Row 4, column 4: .*Triple word$/);
  await saveLayout(page, true);
});
