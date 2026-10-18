import { test, expect } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { App } from "./App";
import { fixtures } from "./fixtures";
import { publicSnapshot } from "./public-state";
import { SharedBoard } from "./SharedBoard";

test("unconfigured entry point explicitly reports unavailable service", () => {
  const endpoint = process.env.VITE_SCRABBLE_ENDPOINT;
  const api = process.env.VITE_SCRABBLE_API;
  delete process.env.VITE_SCRABBLE_ENDPOINT;
  delete process.env.VITE_SCRABBLE_API;
  try {
    const html = renderToStaticMarkup(<App />);
    expect(html).toContain("Scrabble</h1>");
    expect(html).toContain('role="alert"');
    expect(html).toContain("The game service is unavailable.");
    expect(html).not.toContain("<td ");
  } finally {
    if (endpoint !== undefined) {
      process.env.VITE_SCRABBLE_ENDPOINT = endpoint;
    }
    if (api !== undefined) {
      process.env.VITE_SCRABBLE_API = api;
    }
  }
});
for (const phase of ["lobby", "playing", "finished"] as const) {
  test(`public ${phase} fixture renders without private state`, () => {
    const snapshot = fixtures.snapshots[phase];
    const html = renderToStaticMarkup(<SharedBoard snapshot={snapshot} rules={fixtures.ruleset} />);
    expect((html.match(/<td /g) ?? []).length).toBe(225);
    expect(html).toContain(`${snapshot.remaining_tiles} tiles remaining`);
    expect(html).not.toContain("rack");
    if (phase === "playing") {
      expect(html).toContain("blank");
      expect(html).toContain("Ada&#x27;s turn");
    }
    if (phase === "finished") {
      expect(html).toContain("Winner: Ada");
    }
  });
}
test("public boundary rejects rack injection and malformed identifiers", () => {
  expect(publicSnapshot.safeParse({ ...fixtures.snapshots.playing, rack: ["Q"] }).success).toBe(
    false,
  );
  expect(
    publicSnapshot.safeParse({ ...fixtures.snapshots.playing, game_id: "invalid" }).success,
  ).toBe(false);
});

test("finished ties display every winner and final negative scores", () => {
  const snapshot = publicSnapshot.parse({
    ...fixtures.snapshots.finished,
    phase: { kind: "finished", winners: ["1", "2"] },
    players: fixtures.snapshots.finished.players.map((player) => ({ ...player, score: -2 })),
  });
  const html = renderToStaticMarkup(<SharedBoard snapshot={snapshot} rules={fixtures.ruleset} />);
  expect(html).toContain("Tie: Ada, Lin");
  expect((html.match(/-2 points/g) ?? []).length).toBe(2);
  expect(html).toContain("Game finished");
});

test("shared board renders only committed tiles, never a proposed placement", () => {
  const snapshot = fixtures.snapshots.playing;
  // A legacy client's leaked preview field is not part of the public contract (Decision 2.6).
  expect(
    publicSnapshot.safeParse({
      ...snapshot,
      preview: { player_id: "1", turn: "0", tiles: [] },
    }).success,
  ).toBe(false);
  const html = renderToStaticMarkup(<SharedBoard snapshot={snapshot} rules={fixtures.ruleset} />);
  expect(html).toContain("Committed A");
  expect(html).not.toContain("Tentative");
  expect(html).not.toContain("tentative-tile");
});
