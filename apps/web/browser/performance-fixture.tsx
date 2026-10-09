/** Browser-test-only measurements of real Scrabble components; never imported by production. */
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { MoveEditor } from "../src/MoveEditor";
import { SharedBoard } from "../src/SharedBoard";
import { fixtures } from "../src/fixtures";
import privateRack from "../src/fixtures/private-rack.json" with { type: "json" };
import { playerSnapshot, ruleset } from "../src/public-state";

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
    throw new Error(`Missing or disabled performance-fixture control: ${selector}`);
  }
  return button;
}

/**
 * Measures synchronous React refresh, DOM commit and forced layout after a warm mount.
 * This excludes navigation/network/loading and does not claim the frame was painted.
 */
export function measureBoardRefreshes(): number[] {
  const { container, restore } = testContainer("shared-board-benchmark");
  const root = createRoot(container);
  const measurements: number[] = [];
  try {
    flushSync(() => {
      root.render(<SharedBoard snapshot={fixtures.snapshots.lobby} rules={fixtures.ruleset} />);
    });
    for (let index = 0; index < 20; index++) {
      const snapshot = index % 2 === 0 ? fixtures.snapshots.playing : fixtures.snapshots.finished;
      const started = performance.now();
      flushSync(() => {
        root.render(<SharedBoard snapshot={snapshot} rules={fixtures.ruleset} />);
      });
      const table = container.querySelector<HTMLTableElement>("table.board");
      if (table === null || table.querySelectorAll("td").length !== 225) {
        throw new Error(
          `Shared board update rendered ${table?.querySelectorAll("td").length ?? "no table"} of 225 squares; markup: ${container.innerHTML.slice(0, 180)}`,
        );
      }
      // Force style/layout inside the measured interval, not just React's render phase.
      table.getBoundingClientRect();
      if (container.querySelectorAll(".letter-tile").length !== snapshot.board.length) {
        throw new Error("Shared board contents differ from the authoritative fixture");
      }
      measurements.push(performance.now() - started);
    }
  } finally {
    flushSync(() => root.unmount());
    restore();
  }
  return measurements;
}

/**
 * Measures the browser-local edit-to-visible-draft boundary. Server acknowledgement,
 * authentication and networking have separate correctness/integration tests.
 */
export function measureWordDraftInteractions(): { placeMs: number[]; resetMs: number[] } {
  const snapshot = playerSnapshot.parse(privateRack.snapshots.playing);
  const rules = ruleset.parse(privateRack.ruleset);
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
  try {
    flushSync(() => {
      root.render(
        <MoveEditor snapshot={snapshot} rules={rules} canAct onTurnAction={async () => {}} />,
      );
    });
    const grid = container.querySelector(".phone-board-grid");
    if (grid === null || grid.querySelectorAll("button").length !== 225) {
      throw new Error("Move editor did not render the full interactive board");
    }

    for (let index = 0; index < 12; index++) {
      for (const [tileId, squareIndex] of [
        [a.id, 112],
        [h.id, 113],
      ] as const) {
        const started = performance.now();
        flushSync(() =>
          requiredButton(container, `.phone-rack [data-tile-id="${tileId}"] button`).click(),
        );
        flushSync(() => requiredButton(container, `[data-square-index="${squareIndex}"]`).click());
        grid.getBoundingClientRect();
        const square = grid.querySelector(`[data-square-index="${squareIndex}"]`);
        if (square === null || !square.classList.contains("tentative-square")) {
          throw new Error("Placed tile is not visible in the word draft");
        }
        placeMs.push(performance.now() - started);
      }

      if (grid.querySelectorAll(".tentative-square").length !== 2) {
        throw new Error("The draft must contain both letters of the word AH");
      }
      requiredButton(container, ".draft-actions button:nth-child(3)");

      const resetStarted = performance.now();
      flushSync(() => requiredButton(container, ".draft-actions button:nth-child(2)").click());
      grid.getBoundingClientRect();
      if (grid.querySelectorAll(".tentative-square").length !== 0) {
        throw new Error("Cancel move did not restore the empty draft");
      }
      resetMs.push(performance.now() - resetStarted);
    }
  } finally {
    flushSync(() => root.unmount());
    restore();
  }
  return { placeMs, resetMs };
}
