/** Browser-test-only measurements of real Scrabble components; never imported by production. */
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { MoveEditor } from "../src/MoveEditor";
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
 * Measure an actual mounted SharedBoard React refresh, using the live language
 * control to change every square's accessible description. Unlike DOM inspection
 * of a settled tree, this includes state reconciliation, the commit and layout.
 * Navigation, network, initial mount and final screen paint are excluded.
 */
export async function measureBoardRefreshes(): Promise<number[]> {
  const board = document.querySelector<HTMLTableElement>("table.board");
  const language = document.querySelector<HTMLSelectElement>(".app-preferences select");
  const setValue = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set;
  if (board === null || language === null || setValue === undefined) {
    throw new Error("The real board and its language control must already be mounted");
  }
  if (board.querySelectorAll("tbody td").length !== 225) {
    throw new Error("The 225-square Scrabble board was not mounted");
  }
  const measurements: number[] = [];
  for (let index = 0; index < 20; index++) {
    const nextLocale = index % 2 === 0 ? "de" : "en";
    const started = performance.now();
    // The native setter is needed for React's tracked onChange to observe the event.
    setValue.call(language, nextLocale);
    language.dispatchEvent(new Event("change", { bubbles: true }));
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    board.getBoundingClientRect();
    const elapsed = performance.now() - started;
    if (document.documentElement.lang !== nextLocale) {
      throw new Error("The real board did not commit the requested language update");
    }
    if (board.querySelectorAll("tbody td").length !== 225) {
      throw new Error("Refreshing the board lost squares");
    }
    measurements.push(elapsed);
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
        const rackButton = requiredButton(container, `.phone-rack [data-tile-id="${tileId}"] button`);
        const squareButton = requiredButton(container, `[data-square-index="${squareIndex}"]`);
        const started = performance.now();
        flushSync(() => rackButton.click());
        flushSync(() => squareButton.click());
        grid.getBoundingClientRect();
        const elapsed = performance.now() - started;
        const square = grid.querySelector(`[data-square-index="${squareIndex}"]`);
        if (square === null || !square.classList.contains("tentative-square")) {
          throw new Error("Placed tile is not visible in the word draft");
        }
        placeMs.push(elapsed);
      }

      if (grid.querySelectorAll(".tentative-square").length !== 2) {
        throw new Error("The draft must contain both letters of the word AH");
      }
      requiredButton(container, ".draft-actions button:nth-child(3)");

      const resetButton = requiredButton(container, ".draft-actions button:nth-child(2)");
      const resetStarted = performance.now();
      flushSync(() => resetButton.click());
      grid.getBoundingClientRect();
      const resetElapsed = performance.now() - resetStarted;
      if (grid.querySelectorAll(".tentative-square").length !== 0) {
        throw new Error("Cancel move did not restore the empty draft");
      }
      resetMs.push(resetElapsed);
    }
  } finally {
    flushSync(() => root.unmount());
    restore();
  }
  return { placeMs, resetMs };
}
