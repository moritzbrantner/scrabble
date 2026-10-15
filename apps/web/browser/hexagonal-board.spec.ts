// Independent #96 integration acceptance. Current native hex correctness is
// reused; this file checks actual catalog/UI/server exposure, never a fake game.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { z } from "zod";
import { faceText } from "../src/public-state";
import { observeSnapshots } from "./pages-observer";
import { expect, test, type Locator, type Page } from "@playwright/test";

import { gameFixture, selectOpening } from "./game-fixture";

const query = new URLSearchParams({
  server: gameFixture.endpoint,
  api: gameFixture.api,
  certificate: Buffer.from(gameFixture.certificateHash).toString("hex"),
});

async function previewCells(preview: Locator) {
  return preview.getByRole("cell").or(preview.getByRole("gridcell"));
}

for (const width of [1280, 390]) {
  test(`real hex sample boards preserve independent choices and preview on ${width}px`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`./?${query}`);
    const wordList = page.getByRole("combobox", { name: "Word list", exact: true });
    const tileProfile = page.getByRole("combobox", { name: "Tile profile", exact: true });
    const boardLayout = page.getByRole("combobox", { name: "Board layout", exact: true });
    await expect(boardLayout).toBeVisible();
    const originalWord = await wordList.inputValue();
    const originalTiles = await tileProfile.inputValue();
    const hexOptions = boardLayout.getByRole("option").filter({ hasText: /hex/i });
    expect(
      await hexOptions.count(),
      "the real catalog offers independently selectable hex sample configurations for both policies",
    ).toBeGreaterThanOrEqual(2);
    const candidates = await hexOptions.evaluateAll((elements) =>
      elements.map((element) => ({
        value: (element as HTMLOptionElement).value,
        label: element.textContent ?? "",
      })),
    );
    // Do not prescribe exact profile names or wording. Both existing policies
    // must be discoverable as independent board choices rather than tile swaps.
    expect(candidates.some((entry) => /main.*word/i.test(entry.label))).toBe(true);
    expect(candidates.some((entry) => /all.*word/i.test(entry.label))).toBe(true);
    for (const candidate of candidates) {
      await boardLayout.selectOption(candidate.value);
      expect(await wordList.inputValue()).toBe(originalWord);
      expect(await tileProfile.inputValue()).toBe(originalTiles);
      const preview = page
        .getByRole("table", { name: "Board preview", exact: true })
        .or(page.getByRole("grid", { name: "Board preview", exact: true }));
      await expect(preview).toBeVisible();
      const side = Number(await preview.getAttribute("data-board-size"));
      expect(side % 2).toBe(1);
      expect(side).toBeGreaterThanOrEqual(3);
      expect(side).toBeLessThanOrEqual(15);
      const radius = Math.floor(side / 2);
      const cells = await previewCells(preview);
      await expect(cells).toHaveCount(3 * radius * radius + 3 * radius + 1);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
        width,
      );
      await testInfo.attach(`hex-choice-${width}.png`, {
        body: await page.screenshot(),
        contentType: "image/png",
      });
    }
  });
}

// Expected words and scores are pinned by the independent legal native trace in
// multi_letter_tiles.rs. Browser commands below construct that board through play.
const directory = dirname(process.env.SCRABBLE_GAME_FIXTURE!);
const executable = resolve(
  process.env.CARGO_TARGET_DIR ?? "target",
  "debug/examples/browser_game_fixture",
);
const opening = z.strictObject({ requestId: z.string(), requestedAt: z.number() });
const analysis = (page: Page) =>
  page
    .getByRole("region", { name: /preview|proposed|analysis|breakdown/i })
    .filter({ hasText: /main\s+word/i });

async function unblocked(target: Locator) {
  await target.scrollIntoViewIfNeeded();
  expect(
    await target.evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      const hit = document.elementFromPoint(
        bounds.x + bounds.width / 2,
        bounds.y + bounds.height / 2,
      );
      return hit !== null && (hit === element || element.contains(hit));
    }),
    "the real pointer centre hits its intended control",
  ).toBe(true);
}
const sessionIdentity = (page: Page) =>
  page.evaluate(() => {
    const identity: Record<string, unknown> = {};
    const sequences: Record<string, number> = {};
    const capabilities: Record<string, string> = {};
    for (const key of Object.keys(sessionStorage)) {
      const raw = sessionStorage.getItem(key);
      let value: unknown = raw;
      try {
        value = JSON.parse(raw ?? "null");
      } catch {
        /* compare non-JSON verbatim */
      }
      if (value !== null && typeof value === "object" && !Array.isArray(value)) {
        const { sequence, token, ...rest } = value as Record<string, unknown>;
        if (typeof token === "string") {
          capabilities[key] = token;
        }
        if (typeof sequence === "number") {
          sequences[key] = sequence;
        }
        value = rest;
      }
      identity[key] = value;
    }
    return { identity, sequences, capabilities };
  });
async function expectFaceFits(scope: Locator, face: string, where: string) {
  const found = await scope.evaluate((root, face) => {
    const results: { inside: boolean; size: number; detail: string }[] = [];
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      const element = node.parentElement;
      if (node.textContent?.trim() !== face || element === null) {
        continue;
      }
      const tile = element.closest(
        ".rack-tile, .letter-tile, .phone-square, .tile-preview-square, button, td, li",
      );
      const range = document.createRange();
      range.selectNodeContents(node);
      const text = range.getBoundingClientRect();
      if (tile === null || text.width === 0 || text.height === 0) {
        continue;
      }
      const box = tile.getBoundingClientRect();
      results.push({
        inside:
          text.left >= box.left - 0.5 &&
          text.right <= box.right + 0.5 &&
          text.top >= box.top - 0.5 &&
          text.bottom <= box.bottom + 0.5,
        size: Number.parseFloat(getComputedStyle(element).fontSize),
        detail: `text ${JSON.stringify(text)} tile ${JSON.stringify(box)}`,
      });
    }
    return results;
  }, face);
  expect(found.length, `${where}: a visible ${face} face`).toBeGreaterThan(0);
  for (const result of found) {
    expect(result.inside, `${where}: ${face} fits its tile (${result.detail})`).toBe(true);
    expect(result.size, `${where}: ${face} is legible`).toBeGreaterThanOrEqual(9);
  }
}

async function tabTo(page: Page, target: Locator) {
  for (let count = 0; count < 80; count += 1) {
    if (await target.evaluate((element) => element === document.activeElement)) {
      return;
    }
    await page.keyboard.press("Tab");
  }
  throw new Error("Gameplay control is not keyboard reachable");
}
async function activate(page: Page, target: Locator, touch: boolean) {
  if (touch) {
    await unblocked(target);
    await target.tap();
  } else {
    await tabTo(page, target);
    await page.keyboard.press("Enter");
  }
}
async function place(
  page: Page,
  cells: readonly (readonly [number, number, string])[],
  touch: boolean,
  blankAs = "A",
) {
  const rack = page.getByRole("list", { name: "Your rack", exact: true });
  const editor = page.getByRole("region", { name: "Move editor", exact: true });
  for (const [row, column, face] of cells) {
    // Placed physical tiles have a different accessible name. Exact unplaced
    // labels select an owned physical tile once, without changing native racks.
    const available = rack
      .getByRole("button", {
        name: face === "?" ? /^Blank tile/ : new RegExp(`^${face}, \\d+ points$`),
      })
      .first();
    await activate(page, available, touch);
    const target = editor.getByRole("button", {
      name: new RegExp(`^Row ${row + 1}, column ${column + 1}:`),
    });
    if (touch) {
      await unblocked(target);
      await target.tap();
    } else {
      const selected = editor.locator('[data-square-index][tabindex="0"]');
      await tabTo(page, selected);
      const index = Number(await selected.getAttribute("data-square-index"));
      let currentRow = Math.floor(index / 7);
      let currentColumn = index % 7;
      // Every column intersects the centre row and every row intersects the
      // centre column. This route never assumes the hex corners are playable.
      for (const [nextRow, nextColumn] of [
        [3, currentColumn],
        [3, 3],
        [row, 3],
        [row, column],
      ]) {
        while (currentRow !== nextRow) {
          await page.keyboard.press(currentRow > nextRow! ? "ArrowUp" : "ArrowDown");
          currentRow += currentRow > nextRow! ? -1 : 1;
        }
        while (currentColumn !== nextColumn) {
          await page.keyboard.press(currentColumn > nextColumn! ? "ArrowLeft" : "ArrowRight");
          currentColumn += currentColumn > nextColumn! ? -1 : 1;
        }
      }
      await expect(target).toBeFocused();
      await page.keyboard.press("Enter");
    }
    if (face === "?") {
      await activate(
        page,
        page
          .getByRole("group", { name: "Choose blank letter", exact: true })
          .getByRole("button", { name: blankAs, exact: true }),
        touch,
      );
    }
  }
}
async function expectBreakdown(
  page: Page,
  main: string,
  metric: string,
  words: readonly (readonly [string, number])[],
  total: number,
) {
  const explanation = analysis(page);
  await expect(explanation).toHaveCount(1);
  await expect(explanation).toContainText(new RegExp(`main\\s+word[\\s\\S]*\\b${main}\\b`, "i"));
  await expect(explanation).toContainText(
    metric === "tiles"
      ? /(?:length|metric)[\s\S]*(?:tiles|cells)/i
      : /(?:length|metric)[\s\S]*letters/i,
  );
  const rows = explanation.getByRole("row").or(explanation.getByRole("listitem"));
  for (const word of new Set(words.map(([word]) => word))) {
    const matching = rows.filter({ hasText: new RegExp(`\\b${word}\\b`) });
    const scores = words.filter(([candidate]) => candidate === word).map(([, score]) => score);
    await expect(matching).toHaveCount(scores.length);
    await expect(matching).toContainText(scores.map((score) => new RegExp(`\\b${score}\\b`)));
  }
  await expect(explanation).toContainText(
    new RegExp(`(?:total|move score)[\\s\\S]*\\b${total}\\b`, "i"),
  );
}

for (const policy of ["all_word", "main_word_only"] as const) {
  for (const metric of ["tiles", "letters"] as const) {
    test(`native hex ${policy}/${metric} preserves authoritative play, preview and recovery`, async ({
      page,
      browser,
    }, testInfo) => {
      test.setTimeout(180_000);
      const desktop = await browser.newContext({
        viewport: { width: 1280, height: 900 },
        reducedMotion: "reduce",
      });
      const context = await browser.newContext({
        viewport: { width: 390, height: 844 },
        hasTouch: true,
        reducedMotion: "reduce",
      });
      const ada = await desktop.newPage();
      const lin = await context.newPage();
      const boardState = await observeSnapshots(page, "1", true);
      const adaState = await observeSnapshots(ada, "2");
      const linState = await observeSnapshots(lin, "3");
      try {
        await page.goto(`./?${query}`);
        await page
          .getByRole("combobox", { name: "Word list", exact: true })
          .selectOption("acceptance-hex-words");
        await page
          .getByRole("combobox", { name: "Tile profile", exact: true })
          .selectOption("acceptance-multi-letter-tiles");
        await page
          .getByRole("combobox", { name: "Board layout", exact: true })
          .selectOption(`acceptance-hex-${policy}-${metric}`);
        await page.getByText("Advanced match rules", { exact: true }).click();
        await page.getByLabel("Scoreless-turn limit", { exact: true }).fill("2");
        const deal = opening.parse(
          JSON.parse(
            execFileSync(executable, ["--hex-opening", directory], {
              encoding: "utf8",
              timeout: 30_000,
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
        for (const [phone, name] of [
          [ada, "Ada"],
          [lin, "Lin"],
        ] as const) {
          await phone.goto(href);
          await phone.getByLabel("Player name", { exact: true }).fill(name);
          await phone.getByRole("button", { name: "Join game", exact: true }).click();
        }
        await page.getByRole("button", { name: "Start game", exact: true }).click();
        await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
        await expect.poll(() => linState()?.own_rack.tiles.length).toBe(7);
        const count = (snapshot: ReturnType<typeof adaState>, face: string) =>
          snapshot?.own_rack.tiles.filter((tile) => faceText(tile.face) === face).length ?? 0;
        expect(count(adaState(), "A")).toBeGreaterThanOrEqual(2);
        expect(count(adaState(), "CH")).toBeGreaterThanOrEqual(1);
        expect(count(adaState(), "SCH")).toBeGreaterThanOrEqual(1);
        expect(count(linState(), "A")).toBeGreaterThanOrEqual(3);
        expect(linState()?.own_rack.tiles.some((tile) => tile.face.kind === "blank")).toBe(true);
        expect(boardState()?.own_rack.tiles).toEqual([]);
        const nativeRules = boardState()!.public.configuration;
        expect(nativeRules.tiles.find((tile) => faceText(tile.face) === "CH")?.value).toBe(5);
        expect(nativeRules.tiles.find((tile) => faceText(tile.face) === "SCH")?.value).toBe(8);
        // Inspect the actual server-projected rules. These names belong to the
        // existing native Ruleset, not a second browser scoring model.
        const geometry = z
          .object({
            board_shape: z.literal("hexagon"),
            board_size: z.literal(7),
            premiums: z.array(z.unknown()).length(37),
            premium_policy: z.string().default("all_word"),
            main_word_length: z.string().default("tiles"),
          })
          .parse(nativeRules);
        expect(geometry.premium_policy).toBe(policy);
        expect(geometry.main_word_length).toBe(metric);
        for (const client of [ada, lin]) {
          const cells = client
            .getByRole("region", { name: "Move editor", exact: true })
            .getByRole("button", { name: /^Row \d+, column \d+:/ });
          await expect(cells).toHaveCount(37);
          await expect(client.getByRole("button", { name: /^Row 4, column 4:/ })).toBeVisible();
        }
        await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
        await ada.setViewportSize({ width: 1280, height: 900 });
        const before = structuredClone(boardState()!.public);
        await place(
          ada,
          [
            [0, 3, "SCH"],
            [0, 4, "A"],
          ],
          false,
        );
        const commit = ada.getByRole("button", { name: "Commit move", exact: true });
        if (await commit.isEnabled()) {
          await activate(ada, commit, false);
        }
        await expect(ada.getByRole("region", { name: "Move editor", exact: true })).toContainText(
          /cent(?:er|re)/i,
        );
        expect(boardState()!.public.board).toEqual(before.board);
        await activate(ada, ada.getByRole("button", { name: "Cancel move", exact: true }), false);
        await expect.poll(() => boardState()?.public.preview).toBeNull();

        let total = 24;
        if (policy === "main_word_only") {
          total = metric === "tiles" ? 15 : 21;
        }
        const main = metric === "tiles" ? "AAA" : "ASCH";
        const breakdown =
          metric === "tiles"
            ? ([
                ["AAA", 6],
                ["ASCH", policy === "all_word" ? 18 : 9],
              ] as const)
            : ([
                ["ASCH", 18],
                ["AAA", policy === "all_word" ? 6 : 3],
              ] as const);
        const moves = [
          {
            actor: ada,
            cells: [
              [3, 3, "SCH"],
              [3, 4, "A"],
            ] as const,
            touch: false,
            main: "SCHA",
            words: [["SCHA", 9]] as const,
            total: 9,
          },
          {
            actor: lin,
            cells: [
              [2, 4, "A"],
              [2, 5, "A"],
            ] as const,
            touch: true,
            main: "AA",
            words: [
              ["AA", 2],
              ["AA", 2],
              ["ASCH", 9],
              ["AA", 2],
            ] as const,
            total: 15,
          },
          {
            actor: ada,
            cells: [[2, 3, "A"]] as const,
            touch: false,
            main,
            words: breakdown,
            total,
          },
          {
            actor: lin,
            cells: [[2, 6, "A"]] as const,
            touch: true,
            main: "AAAA",
            words: [["AAAA", 4]] as const,
            total: 4,
          },
          {
            actor: ada,
            cells: [[4, 3, "CH"]] as const,
            touch: false,
            main: "ASCHCH",
            words: [
              ["ASCHCH", 14],
              ["AACH", 7],
            ] as const,
            total: 21,
          },
          {
            actor: lin,
            cells: [[4, 2, "?"]] as const,
            touch: true,
            main: "ASCHA",
            words: [
              ["ASCHA", 9],
              ["ACH", 5],
            ] as const,
            total: 14,
          },
        ];
        for (const [index, move] of moves.entries()) {
          if (index === 1) {
            const unchanged = structuredClone(boardState()!.public);
            const rack = structuredClone(linState()!.own_rack);
            await place(
              lin,
              [
                [2, 4, "A"],
                [2, 5, "?"],
              ],
              true,
              "T",
            );
            const submit = lin.getByRole("button", { name: "Commit move", exact: true });
            if (await submit.isEnabled()) {
              await activate(lin, submit, true);
            }
            await expect(
              lin.getByRole("region", { name: "Move editor", exact: true }),
            ).toContainText(/not[\s\S]*(?:dictionary|valid)|invalid[\s\S]*word/i);
            expect(boardState()!.public.board).toEqual(unchanged.board);
            expect(boardState()!.public.players).toEqual(unchanged.players);
            expect(boardState()!.public.phase).toEqual(unchanged.phase);
            expect(boardState()!.public.history).toEqual(unchanged.history);
            expect(linState()!.own_rack).toEqual(rack);
            await activate(
              lin,
              lin.getByRole("button", { name: "Cancel move", exact: true }),
              true,
            );
            await expect.poll(() => boardState()?.public.preview).toBeNull();
          }
          // The phone also exposes the combined-face metric comparison in
          // landscape, while Ada remains a genuine non-touch desktop context.
          if (index === 3) {
            await lin.setViewportSize({ width: 844, height: 390 });
          }
          const untouched = structuredClone(boardState()!.public);
          await place(move.actor, move.cells, move.touch);
          await expect
            .poll(() => boardState()?.public.preview?.tiles.length)
            .toBe(move.cells.length);
          for (const client of [page, move.actor]) {
            await expectBreakdown(client, move.main, metric, move.words, move.total);
          }
          if (index === 4 && metric === "tiles") {
            for (const client of [page, move.actor]) {
              await expect(analysis(client)).toContainText(
                /tie|priority|vertical[\s\S]*diagonal|diagonal[\s\S]*vertical/i,
              );
            }
          }
          if (index === 5) {
            for (const [row, column, face] of [
              [3, 3, "SCH"],
              [4, 3, "CH"],
            ] as const) {
              const cell = lin
                .getByRole("region", { name: "Move editor", exact: true })
                .getByRole("button", {
                  name: new RegExp(`^Row ${row + 1}, column ${column + 1}:`),
                });
              await expectFaceFits(cell, face, "landscape hex committed tile");
              await unblocked(cell);
            }
          }
          if (move.actor === lin) {
            expect(
              await lin.evaluate(() => document.documentElement.scrollWidth),
            ).toBeLessThanOrEqual(lin.viewportSize()!.width);
          }
          if (index === 0) {
            await activate(
              ada,
              ada.getByRole("button", { name: "Cancel move", exact: true }),
              false,
            );
            await expect.poll(() => boardState()?.public.preview).toBeNull();
            for (const client of [page, ada]) {
              await expect(analysis(client).filter({ hasText: /\bSCHA\b/ })).toHaveCount(0);
            }
            expect(boardState()!.public.board).toEqual(untouched.board);
            await place(ada, move.cells, false);
            await expect
              .poll(() => boardState()?.public.preview?.tiles.length)
              .toBe(move.cells.length);
            for (const client of [page, ada]) {
              await expectBreakdown(client, move.main, metric, move.words, move.total);
            }
          }
          expect(boardState()!.public.board).toEqual(untouched.board);
          expect(boardState()!.public.players).toEqual(untouched.players);
          expect(boardState()!.public.phase).toEqual(untouched.phase);
          await testInfo.attach(`hex-${policy}-${metric}-turn-${index + 1}-preview.png`, {
            body: await move.actor.screenshot(),
            contentType: "image/png",
          });
          await activate(
            move.actor,
            move.actor.getByRole("button", { name: "Commit move", exact: true }),
            move.touch,
          );
          await expect.poll(() => boardState()?.public.history.length).toBe(index + 1);
          const result = boardState()!.public.history[index]!.action;
          expect(result).toMatchObject({
            kind: "commit",
            words: move.words.map((word) => word[0]),
            move_score: move.total,
            blank_count: index === 5 ? 1 : 0,
          });
          expect(boardState()?.own_rack.tiles).toEqual([]);
        }
        const savedPublic = structuredClone(boardState()!.public);
        const savedRack = structuredClone(adaState()!.own_rack);
        const identity = await sessionIdentity(ada);
        await ada.reload();
        await expect(ada.getByText("Connected · Player 2", { exact: true })).toBeVisible();
        await expect.poll(() => adaState()?.own_rack).toEqual(savedRack);
        expect(adaState()!.public.history).toEqual(savedPublic.history);
        for (const client of [page, ada, lin]) {
          const history = client.getByRole("region", { name: /history/i });
          const third = history.getByRole("listitem").filter({ hasText: /Turn 3:/ });
          await expect(third).toHaveCount(1);
          await expect(third).toContainText(new RegExp(`main\\s+word[\\s\\S]*\\b${main}\\b`, "i"));
          await expect(third).toContainText(
            metric === "tiles"
              ? /(?:metric|length)[\s\S]*(?:tiles|cells)/i
              : /(?:metric|length)[\s\S]*letters/i,
          );
          for (const [word, score] of breakdown) {
            await expect(third).toContainText(new RegExp(`\\b${word}\\b[\\s\\S]*\\b${score}\\b`));
          }
          const fifth = history.getByRole("listitem").filter({ hasText: /Turn 5:/ });
          await expect(fifth).toContainText(/main\s+word[\s\S]*\bASCHCH\b/i);
          await expect(fifth).toContainText(/\bAACH\b[\s\S]*\b7\b/);
        }
        const restored = await sessionIdentity(ada);
        expect(
          JSON.stringify(restored.identity) === JSON.stringify(identity.identity),
          "reload preserves private session identity",
        ).toBe(true);
        // ADR 0004: reconnect keeps the player/route but rotates its private capability.
        // Compare booleans so a failed assertion never prints either private token.
        expect(Object.keys(identity.capabilities)).toHaveLength(1);
        expect(Object.keys(restored.capabilities)).toEqual(Object.keys(identity.capabilities));
        for (const [key, token] of Object.entries(identity.capabilities)) {
          expect(/^[0-9a-f]{32}$/.test(token), "original resume capability is valid").toBe(true);
          expect(
            /^[0-9a-f]{32}$/.test(restored.capabilities[key] ?? ""),
            "restored resume capability is valid",
          ).toBe(true);
          expect(
            restored.capabilities[key] !== token,
            "reconnect rotates the private resume capability",
          ).toBe(true);
        }
        expect(Object.keys(restored.sequences)).toEqual(Object.keys(identity.sequences));
        for (const [key, sequence] of Object.entries(identity.sequences)) {
          expect(restored.sequences[key]).toBeGreaterThanOrEqual(sequence);
        }
        await activate(
          ada,
          ada.getByRole("button", { name: "Exchange tiles", exact: true }),
          false,
        );
        await activate(
          ada,
          ada.getByRole("list", { name: "Your rack", exact: true }).getByRole("button").first(),
          false,
        );
        await activate(
          ada,
          ada.getByRole("button", { name: "Confirm exchange", exact: true }),
          false,
        );
        await expect.poll(() => boardState()?.public.history.length).toBe(7);
        await expect.poll(() => adaState()?.public.history.length).toBe(7);
        await expect.poll(() => linState()?.public.history.length).toBe(7);
        const rackValue = (snapshot: ReturnType<typeof adaState>) =>
          snapshot!.own_rack.tiles.reduce((sum, tile) => {
            const definition = nativeRules.tiles.find(
              (definition) => faceText(definition.face) === faceText(tile.face),
            );
            expect(definition).toBeDefined();
            return sum + definition!.value;
          }, 0);
        const scores = [30 + total - rackValue(adaState()), 33 - rackValue(linState())];
        await activate(lin, lin.getByRole("button", { name: "Pass turn", exact: true }), true);
        await activate(lin, lin.getByRole("button", { name: "Confirm pass", exact: true }), true);
        await expect.poll(() => boardState()?.public.phase.kind).toBe("finished");
        await expect.poll(() => adaState()?.public).toEqual(boardState()?.public);
        await expect.poll(() => linState()?.public).toEqual(boardState()?.public);
        expect(boardState()!.public.history).toHaveLength(8);
        for (const [index, id] of ["2", "3"].entries()) {
          expect(boardState()!.public.players.find((player) => player.id === id)?.score).toBe(
            scores[index],
          );
        }
        const winners = ["2", "3"].filter((_, index) => scores[index] === Math.max(...scores));
        expect(boardState()!.public.phase).toEqual({ kind: "finished", winners });
        await testInfo.attach(`hex-${policy}-${metric}-finished.png`, {
          body: await page.screenshot(),
          contentType: "image/png",
        });
        const match = new URL(page.url()).searchParams.get("match");
        if (!match) {
          throw new Error("Missing retained match route");
        }
        // Native recovery bundles are materialized only at graceful shutdown.
        // The source-owned runner verifies these SAME completed matches after all
        // browser cases and fixture shutdown, before removing the private fixture.
        const expectations = join(directory, "hex-replay-expectations");
        mkdirSync(expectations, { recursive: true, mode: 0o700 });
        writeFileSync(
          join(expectations, `${policy}-${metric}.json`),
          JSON.stringify({
            match,
            game: {
              game_id: boardState()!.public.game_id,
              phase: boardState()!.public.phase,
              history: boardState()!.public.history,
            },
          }),
          { mode: 0o600 },
        );
        for (const client of [page, ada, lin]) {
          expect((await new AxeBuilder({ page: client }).analyze()).violations).toEqual([]);
        }
      } finally {
        await Promise.all([desktop.close(), context.close()]);
      }
    });
  }
}
