import { test, expect } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { App } from "./App";
import { fixtures } from "./fixtures";
import { publicSnapshot } from "./public-state";
import { SharedBoard } from "./SharedBoard";

test("entry point displays a compact lobby and complete public board", () => {
  const html = renderToStaticMarkup(<App />);
  expect(html).toContain("Scrabble</h1>");
  expect(html).toContain("Waiting for players to join.");
  expect((html.match(/<td /g) ?? []).length).toBe(225);
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
