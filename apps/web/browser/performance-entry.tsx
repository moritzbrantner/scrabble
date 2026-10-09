/** Test-only optimized-bundle entry. Not included in the production Scrabble build. */
import { useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { SharedBoard } from "../src/SharedBoard";
import { fixtures } from "../src/fixtures";
import { PreferencesProvider } from "../src/preferences";
import { measureBoardRefreshes, measureWordDraftInteractions } from "./performance-fixture";
import "../src/styles.css";

type Phase = "playing" | "finished";

declare global {
  interface Window {
    scrabblePerformance?: {
      setBoard: (phase: Phase) => void;
      measureBoardRefreshes: typeof measureBoardRefreshes;
      measureWordDraftInteractions: typeof measureWordDraftInteractions;
    };
  }
}

function BoardHarness() {
  const [phase, setPhase] = useState<Phase>("playing");
  window.scrabblePerformance = {
    // Preserve actual SharedBoard rendering and the React commit, rather
    // than timing an inert DOM query over already rendered cells.
    setBoard: (next) => flushSync(() => setPhase(next)),
    measureBoardRefreshes,
    measureWordDraftInteractions,
  };
  return <SharedBoard snapshot={fixtures.snapshots[phase]} rules={fixtures.ruleset} />;
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
