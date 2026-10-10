/** Test-only optimized-bundle entry. Not included in the production Scrabble build. */
import { useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { SharedBoard } from "../src/SharedBoard";
import { fixtures } from "../src/fixtures";
import privateRack from "../src/fixtures/private-rack.json" with { type: "json" };
import { PreferencesProvider } from "../src/preferences";
import { playerSnapshot, ruleset, type PublicSnapshot } from "../src/public-state";
import { measureBoardRefreshes, measureWordDraftInteractions } from "./performance-fixture";
import {
  populatedPlayerSnapshot,
  type BoardFixturePhase,
} from "./populated-performance-state";
import "../src/styles.css";

const populated = populatedPlayerSnapshot(
  playerSnapshot.parse(privateRack.snapshots.playing),
  ruleset.parse(privateRack.ruleset),
).public;
const populatedUpdated: PublicSnapshot = {
  ...populated,
  revision: String(BigInt(populated.revision) + 1n),
  board: [
    ...populated.board,
    { coordinate: { row: 7, column: 13 }, letter: "A", is_blank: false },
  ],
  remaining_tiles: populated.remaining_tiles - 1,
};
const snapshots: Record<BoardFixturePhase, PublicSnapshot> = {
  playing: fixtures.snapshots.playing,
  finished: fixtures.snapshots.finished,
  populated,
  "populated-updated": populatedUpdated,
};

type BenchmarkApi = {
  setBoard: (phase: BoardFixturePhase) => void;
  measureBoardRefreshes: typeof measureBoardRefreshes;
  measureWordDraftInteractions: typeof measureWordDraftInteractions;
};

function BoardHarness() {
  const [phase, setPhase] = useState<BoardFixturePhase>("playing");
  (window as Window & { scrabblePerformance?: BenchmarkApi }).scrabblePerformance = {
    // Keep production SharedBoard rendering and the actual React commit inside
    // each timer rather than measuring reads of an inert pre-rendered DOM.
    setBoard: (next) => flushSync(() => setPhase(next)),
    measureBoardRefreshes,
    measureWordDraftInteractions,
  };
  return <SharedBoard snapshot={snapshots[phase]} rules={fixtures.ruleset} />;
}

const container = document.getElementById("root");
if (container === null) {
  throw new Error("Missing Scrabble performance fixture root");
}
createRoot(container).render(
  <PreferencesProvider initial={{ locale: "en", theme: "light" }}>
    <BoardHarness />
  </PreferencesProvider>,
);
