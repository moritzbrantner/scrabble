import { expect, type Browser, type Page } from "@playwright/test";

import { freshOpening, gameFixture, selectOpening } from "../../browser/game-fixture";
import { interruptible, observeSnapshots } from "../../browser/pages-observer";
import type { Expected, Transition, TransitionId, View } from "./contract";

const certificate = gameFixture.certificateHash
  .map((byte) => byte.toString(16).padStart(2, "0"))
  .join("");

let previousRequest = gameFixture.opening.requestId;

/**
 * One fresh disposable match with the shared board and Ada's and Lin's phones in
 * separate browser contexts. Observation reads real UI and each recipient's own wire
 * snapshots; nothing is injected into the application.
 */
export async function openSession(browser: Browser, baseURL: string) {
  const contexts = await Promise.all([
    browser.newContext({ baseURL, viewport: { width: 1280, height: 900 } }),
    browser.newContext({ viewport: { width: 390, height: 844 } }),
    browser.newContext({ viewport: { width: 390, height: 844 } }),
  ]);
  const [board, ada, lin] = await Promise.all(contexts.map((context) => context.newPage()));
  if (board === undefined || ada === undefined || lin === undefined) {
    throw new Error("Missing review page");
  }
  const views: Record<View, Page> = { board, ada, lin };
  const boardState = await observeSnapshots(board, "1", true);
  // Ada's second connection is held until the explorer releases its restored snapshot.
  const adaState = await observeSnapshots(ada, "2", false, true);
  const linState = await observeSnapshots(lin, "3");
  await interruptible(ada);
  // Every journey needs its own creation request; a reused one recovers the previous game.
  const opening = freshOpening(previousRequest);
  previousRequest = opening.requestId;
  let invite = "";
  let createdMatch = "";

  const editor = ada.getByRole("region", { name: "Move editor", exact: true });
  const place = async (letters: readonly string[]) => {
    const rack = ada.getByRole("list", { name: "Your rack", exact: true });
    for (const [index, letter] of letters.entries()) {
      await rack
        .getByRole("button", { name: new RegExp(`^${letter},`) })
        .first()
        .click();
      await editor
        .getByRole("button", { name: new RegExp(`^Row 8, column ${8 + index}:`) })
        .click();
    }
  };
  const join = async (phone: Page, name: string) => {
    await phone.goto(invite);
    await phone.getByLabel("Player name", { exact: true }).fill(name);
    await phone.getByRole("button", { name: "Join game", exact: true }).click();
    await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
  };
  const pass = async (phone: Page) => {
    await phone.getByRole("button", { name: "Pass turn", exact: true }).click();
    await phone.getByRole("button", { name: "Confirm pass", exact: true }).click();
  };
  const dispatch = async (event: string) => {
    await ada.evaluate((name) => window.dispatchEvent(new Event(name)), event);
  };
  const playing = () => {
    const phase = boardState()?.public.phase;
    return phase?.kind === "playing" ? phase : undefined;
  };

  const actions: Record<TransitionId, () => Promise<void>> = {
    "create-game": async () => {
      await board.goto(
        `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`,
      );
      await selectOpening(board, opening);
      await board.getByRole("button", { name: "Create game", exact: true }).click();
      invite =
        (await board
          .getByRole("link", { name: "Join this game", exact: true })
          .getAttribute("href")) ?? "";
      createdMatch = new URL(invite).searchParams.get("match") ?? "";
    },
    "reject-malformed-invite": async () => {
      const malformed = new URL(invite);
      malformed.searchParams.set("match", "../bad");
      await ada.goto(malformed.href);
    },
    "ada-joins": () => join(ada, "Ada"),
    "lin-joins": () => join(lin, "Lin"),
    "start-game": async () => {
      await board.getByRole("button", { name: "Start game", exact: true }).click();
      await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
    },
    // The fixture dictionary accepts AT but not TA.
    "commit-rejected-word": async () => {
      await place(["T", "A"]);
      await editor.getByRole("button", { name: "Commit move", exact: true }).click();
    },
    "cancel-draft": () => editor.getByRole("button", { name: "Cancel move", exact: true }).click(),
    "propose-at": () => place(["A", "T"]),
    "commit-at": () => editor.getByRole("button", { name: "Commit move", exact: true }).click(),
    "interrupt-ada": () => dispatch("test-interrupt-transport"),
    "release-snapshot": () => dispatch("test-release-snapshot"),
    "commit-after-reconnect": () =>
      editor.getByRole("button", { name: "Commit move", exact: true }).click(),
    "lin-passes": () => pass(lin),
    "pass-out": async () => {
      const phones = { "2": ada, "3": lin } as Record<string, Page>;
      const start = Number(playing()?.turn ?? 0);
      for (let turn = start; turn < start + 6; turn++) {
        await expect.poll(() => playing()?.turn).toBe(String(turn));
        const phone = phones[playing()?.active_player ?? ""];
        if (phone === undefined) {
          throw new Error("Active player is not a seated phone");
        }
        await pass(phone);
      }
    },
    "start-rematch": () =>
      board.getByRole("button", { name: "Start rematch", exact: true }).click(),
  };

  const seat = { ada: "2", lin: "3" } as const;
  /** Check every declared observable of `expected` plus the standing privacy invariants. */
  const observe = async (expected: Expected, rejection?: Transition["rejection"]) => {
    if (rejection !== undefined) {
      await expect(views[rejection.view].getByText(rejection.text).first()).toBeVisible();
    }
    if (expected.phase !== undefined) {
      await expect.poll(() => boardState()?.public.phase.kind).toBe(expected.phase);
    }
    for (const [view, name] of Object.entries(expected.buttons ?? {}) as [View, string][]) {
      await expect(views[view].getByRole("button", { name, exact: true })).toBeVisible();
    }
    if (expected.active !== undefined) {
      await expect.poll(() => playing()?.active_player).toBe(seat[expected.active]);
    }
    if (expected.turn !== undefined) {
      await expect.poll(() => playing()?.turn).toBe(String(expected.turn));
    }
    if (expected.boardTiles !== undefined) {
      await expect.poll(() => boardState()?.public.board.length).toBe(expected.boardTiles);
    }
    if (expected.seated !== undefined) {
      await expect(
        board.getByRole("list", { name: "Players and scores", exact: true }).getByRole("listitem"),
      ).toHaveCount(expected.seated);
    }
    if (expected.adaDraft !== undefined) {
      await expect(editor.locator(".tentative-square")).toHaveCount(expected.adaDraft);
    }
    if (expected.adaPreview !== undefined) {
      await expect.poll(() => adaState()?.own_preview?.tiles.length ?? 0).toBe(expected.adaPreview);
    }
    if (expected.adaConnection !== undefined) {
      await expect
        .poll(() => ada.evaluate(() => document.documentElement.dataset.connections))
        .toBe("2");
    }
    if (expected.rematchInvite === true) {
      const href = await board
        .getByRole("link", { name: "Join this game", exact: true })
        .getAttribute("href");
      const match = new URL(href ?? "").searchParams.get("match");
      expect(match).toMatch(/^b_/);
      expect(match).not.toBe(createdMatch);
    }
    for (const [view, text] of Object.entries(expected.text ?? {}) as [View, string][]) {
      await expect(views[view].getByText(text).first()).toBeVisible();
    }
    // Decisions 2.5–2.10: drafts never reach the shared board, and Lin never sees one.
    await expect(board.locator(".tentative-tile, .tentative-square")).toHaveCount(0);
    expect(linState()?.own_preview).toBeUndefined();
  };

  return {
    views,
    actions,
    observe,
    close: () => Promise.all(contexts.map((context) => context.close())),
  };
}
