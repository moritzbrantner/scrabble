import { expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";
import { z } from "zod";

import data from "./fixtures/ruleset-variant.json";
import { matchRules } from "./match-rules";
import { draftProblem, editDraft, emptyDraft } from "./move-draft";
import { PlayerRack } from "./PlayerRack";
import { playerSnapshot, ruleset } from "./public-state";
import { SharedBoard } from "./SharedBoard";

const fixture = z
  .strictObject({ ruleset, playing: playerSnapshot, committed: playerSnapshot })
  .parse(data);
const rules = matchRules(fixture.playing.public);

test("the Unicode fixture renders configured board dimensions, premiums and tile values", () => {
  const html = renderToStaticMarkup(
    <SharedBoard snapshot={fixture.committed.public} rules={rules} />,
  );
  expect(html).toContain("7 rows and 7 columns");
  expect([...html.matchAll(/<td /g)]).toHaveLength(49);
  expect(html).toContain("Committed Ö, 0 points, blank");
  expect(html).toContain("Committed L, 2 points");
  const rack = renderToStaticMarkup(
    <PlayerRack rack={fixture.playing.own_rack} rules={rules} canAct={true} />,
  );
  expect([...rack.matchAll(/data-tile-id=/g)]).toHaveLength(3);
  expect(rack).toContain("L, 2 points");
  expect(rack).toContain("Blank tile, 0 points");
});
test("the same draft operations accept the configured blank alphabet and reject unsupported letters", () => {
  const blank = fixture.playing.own_rack.tiles.find((tile) => tile.face.kind === "blank");
  const letter = fixture.playing.own_rack.tiles.find(
    (tile) => tile.face.kind === "letter" && tile.face.letter === "L",
  );
  if (blank === undefined || letter === undefined) {
    throw new Error("Invalid native variant fixture");
  }
  const draft = emptyDraft(fixture.playing);
  const unsupported = editDraft(fixture.playing, rules, draft, {
    kind: "place",
    tileId: blank.id,
    coordinate: { row: 3, column: 3 },
    blankAs: "X",
  });
  expect(unsupported.problem).toBe("blank");
  expect(unsupported.draft).toEqual(draft);
  const first = editDraft(fixture.playing, rules, draft, {
    kind: "place",
    tileId: blank.id,
    coordinate: { row: 3, column: 3 },
    blankAs: "Ö",
  });
  const next = editDraft(fixture.playing, rules, first.draft, {
    kind: "place",
    tileId: letter.id,
    coordinate: { row: 3, column: 4 },
    blankAs: null,
  });
  expect(draftProblem(fixture.playing, rules, next.draft)).toBeUndefined();
});
test("invalid and incomplete public ruleset data fails before presentation", () => {
  const original = fixture.ruleset;
  for (const invalid of [
    { ...original, board_size: 6 },
    { ...original, premiums: [] },
    { ...original, rack_size: 0 },
    { ...original, tiles: [{ face: { kind: "letter", letter: "L" }, count: 201, value: 2 }] },
    { ...original, tiles: [...original.tiles, original.tiles[0]] },
    { ...original, tiles: [{ face: { kind: "blank" }, count: 10, value: 1 }] },
    { ...original, minimum_players: 3 },
    { ...original, bingo_bonus: undefined },
  ]) {
    expect(ruleset.safeParse(invalid).success).toBe(false);
  }
});
