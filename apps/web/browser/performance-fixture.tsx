/** Browser-test-only measurements of real Scrabble components; never imported by production. */
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { MoveEditor } from "../src/MoveEditor";
import { fixtures } from "../src/fixtures";
import privateRack from "../src/fixtures/private-rack.json" with { type: "json" };
import { playerSnapshot, ruleset } from "../src/public-state";
import {
  POPULATED_BOARD_TILE_COUNT,
  POPULATED_DRAFT_SQUARE_INDICES,
  populatedPlayerSnapshot,
  type BoardFixturePhase,
  type BrowserWorkload,
} from "./populated-performance-state";

export type BoardMeasurements = { layoutMs: number[]; frameOpportunityMs: number[] };
export type DraftMeasurements = {
  placeMs: number[];
  resetMs: number[];
  placeFrameOpportunityMs: number[];
  resetFrameOpportunityMs: number[];
};

function testContainer(className: string) {
  const original = document.getElementById("root");
  if (original !== null) {
    original.hidden = true;
  }
  const container = document.createElement("main");
  container.className = className;
  document.body.append(container);
  return {
    container,
    restore: () => {
      container.remove();
      if (original !== null) {
        original.hidden = false;
      }
    },
  };
}

function requiredButton(container: Element, selector: string): HTMLButtonElement {
  const button = container.querySelector<HTMLButtonElement>(selector);
  if (button === null || button.disabled) {
    throw new Error("Missing or disabled performance-fixture control: " + selector);
  }
  return button;
}

/** Two animation frames allow at least one rendering opportunity after a React commit.
 * This is a frame-scheduling proxy, NOT observed physical display paint or touch latency.
 */
function frameOpportunity(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });
}

/**
 * Measure actual SharedBoard updates in an optimized test build.
 * Navigation, fixture preparation and correctness assertions are outside the timer.
 */
export async function measureBoardRefreshes(
  workload: BrowserWorkload = "opening",
  includeFrame = false,
): Promise<BoardMeasurements> {
  const table = document.querySelector<HTMLTableElement>("table.board");
  const setBoard = (
    window as Window & {
      scrabblePerformance?: { setBoard: (phase: BoardFixturePhase) => void };
    }
  ).scrabblePerformance?.setBoard;
  if (
    table === null ||
    setBoard === undefined ||
    table.querySelectorAll("tbody td").length !== 225
  ) {
    throw new Error("The mounted production SharedBoard is unavailable");
  }
  const initial: BoardFixturePhase = workload === "opening" ? "playing" : "populated";
  const changed: BoardFixturePhase = workload === "opening" ? "finished" : "populated-updated";
  setBoard(initial); // Exclude the first transition to the populated fixture from samples.
  const layoutMs: number[] = [];
  const frameOpportunityMs: number[] = [];
  for (let index = 0; index < 20; index++) {
    const phase = index % 2 === 0 ? changed : initial;
    const started = performance.now();
    setBoard(phase);
    table.getBoundingClientRect(); // React reconciliation + DOM commit + layout.
    layoutMs.push(performance.now() - started);
    if (includeFrame) {
      await frameOpportunity();
      frameOpportunityMs.push(performance.now() - started);
    }
    const expectedCount =
      workload === "opening"
        ? fixtures.snapshots[phase === "finished" ? "finished" : "playing"].board.length
        : POPULATED_BOARD_TILE_COUNT + (phase === "populated-updated" ? 1 : 0);
    if (
      table.querySelectorAll("tbody td").length !== 225 ||
      table.querySelectorAll(".letter-tile").length !== expectedCount
    ) {
      throw new Error("A board refresh lost squares or mismatched authoritative tiles");
    }
    if (workload === "populated") {
      const addedSquare = table.tBodies.item(0)?.rows.item(7)?.cells.item(14);
      const hasAddedTile = addedSquare?.querySelector(".letter-tile") !== null;
      if (!addedSquare || hasAddedTile !== (phase === "populated-updated")) {
        throw new Error("The populated snapshot did not update the expected board square");
      }
    }
  }
  return { layoutMs, frameOpportunityMs };
}

/**
 * Measures click dispatch -> synchronous React DOM/layout and, optionally,
 * dispatch -> second requestAnimationFrame. The latter observes a frame
 * opportunity after the commit, not a physical paint or hardware input event.
 */
export async function measureWordDraftInteractions(
  workload: BrowserWorkload = "opening",
  includeFrame = false,
): Promise<DraftMeasurements> {
  const opening = playerSnapshot.parse(privateRack.snapshots.playing);
  const rules = ruleset.parse(privateRack.ruleset);
  const snapshot = workload === "opening" ? opening : populatedPlayerSnapshot(opening, rules);
  const squares = workload === "opening" ? ([112, 113] as const) : POPULATED_DRAFT_SQUARE_INDICES;
  const a = snapshot.own_rack.tiles.find(
    (tile) => tile.face.kind === "letter" && tile.face.letter === "A",
  );
  const h = snapshot.own_rack.tiles.find(
    (tile) => tile.face.kind === "letter" && tile.face.letter === "H",
  );
  if (a === undefined || h === undefined) {
    throw new Error("The authored playing rack must contain A and H");
  }

  const { container, restore } = testContainer("player-phone");
  const root = createRoot(container);
  const placeMs: number[] = [];
  const resetMs: number[] = [];
  const placeFrameOpportunityMs: number[] = [];
  const resetFrameOpportunityMs: number[] = [];
  try {
    flushSync(() => {
      root.render(
        <MoveEditor snapshot={snapshot} rules={rules} canAct onTurnAction={async () => {}} />,
      );
    });
    const grid = container.querySelector(".phone-board-grid");
    if (
      grid === null ||
      grid.querySelectorAll("button").length !== 225 ||
      grid.querySelectorAll(".committed-square").length !== snapshot.public.board.length
    ) {
      throw new Error("Move editor did not render the expected committed board");
    }

    for (let index = 0; index < 12; index++) {
      for (const [tileId, squareIndex] of [
        [a.id, squares[0]],
        [h.id, squares[1]],
      ] as const) {
        const rackButton = requiredButton(
          container,
          '.phone-rack [data-tile-id="' + tileId + '"] button',
        );
        const squareButton = requiredButton(container, '[data-square-index="' + squareIndex + '"]');
        const started = performance.now();
        flushSync(() => rackButton.click());
        flushSync(() => squareButton.click());
        grid.getBoundingClientRect();
        placeMs.push(performance.now() - started);
        if (includeFrame) {
          await frameOpportunity();
          placeFrameOpportunityMs.push(performance.now() - started);
        }
        const square = grid.querySelector('[data-square-index="' + squareIndex + '"]');
        if (square === null || !square.classList.contains("tentative-square")) {
          throw new Error("Placed tile is not visible in the word draft");
        }
      }

      if (
        grid.querySelectorAll(".tentative-square").length !== 2 ||
        grid.querySelectorAll(".committed-square").length !== snapshot.public.board.length
      ) {
        throw new Error("Draft edits must preserve the full committed board and both letters");
      }
      requiredButton(container, ".draft-actions button:nth-child(3)");

      const resetButton = requiredButton(container, ".draft-actions button:nth-child(2)");
      const resetStarted = performance.now();
      flushSync(() => resetButton.click());
      grid.getBoundingClientRect();
      resetMs.push(performance.now() - resetStarted);
      if (includeFrame) {
        await frameOpportunity();
        resetFrameOpportunityMs.push(performance.now() - resetStarted);
      }
      if (
        grid.querySelectorAll(".tentative-square").length !== 0 ||
        grid.querySelectorAll(".committed-square").length !== snapshot.public.board.length
      ) {
        throw new Error("Cancel move did not restore the original committed board");
      }
    }
  } finally {
    flushSync(() => root.unmount());
    restore();
  }
  return { placeMs, resetMs, placeFrameOpportunityMs, resetFrameOpportunityMs };
}
