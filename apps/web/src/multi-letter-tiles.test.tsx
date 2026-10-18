// Independently authored #98 client/protocol acceptance (docs/specs/multi-letter-browser.md).
// Wire shapes follow docs/specs/multi-letter-tiles.md, proven natively by #116. Fixtures are
// parsed from untyped JSON so these tests type-check before and after the schema change.
import { expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import data from "./fixtures/private-rack.json";
import { draftProblem, editDraft, emptyDraft } from "./move-draft";
import { PlayerRack } from "./PlayerRack";
import { profileCatalog } from "./profile-model";
import { playerSnapshot, publicSnapshot, ruleset, tileFace } from "./public-state";
import { SharedBoard } from "./SharedBoard";

/** `"?"` is a blank, one character a single letter, anything else a multi-letter face. */
function face(spelling: string): { kind: string; letter?: string; letters?: string } {
  if (spelling === "?") {
    return { kind: "blank" };
  }
  if (Array.from(spelling).length === 1) {
    return { kind: "letter", letter: spelling };
  }
  return { kind: "letters", letters: spelling };
}
const sampleTiles = (
  [
    ["A", 6, 1],
    ["E", 4, 1],
    ["N", 2, 1],
    ["S", 2, 1],
    ["T", 4, 1],
    ["C", 1, 3],
    ["H", 1, 2],
    ["CH", 4, 5],
    ["SCH", 3, 8],
    ["?", 2, 0],
  ] as const
).map(([spelling, count, value]) => ({ face: face(spelling), count, value }));
const rawRules = { ...data.ruleset, tiles: sampleTiles };
const rawRack = [
  ["CH", "10"],
  ["SCH", "11"],
  ["?", "12"],
  ["A", "13"],
  ["T", "14"],
  ["H", "15"],
  ["E", "16"],
].map(([spelling, id]) => ({ id, face: face(spelling ?? "") }));
function rawSnapshot(overrides: Record<string, unknown> = {}) {
  const playing = structuredClone(data.snapshots.playing);
  return {
    ...playing,
    public: { ...playing.public, configuration: rawRules, ...overrides },
    own_rack: { ...playing.own_rack, tiles: rawRack },
  };
}
const boardTile = (letter: string, row: number, column: number, isBlank = false) => ({
  coordinate: { row, column },
  letter,
  is_blank: isBlank,
});

const committed = (words: string[]) => [
  {
    turn: "0",
    player_id: "1",
    action: { kind: "commit", words, move_score: 14, blank_count: 0 },
    scores: [{ player_id: "1", delta: 14, score: 14 }],
  },
];

test("tile faces accept bounded uppercase multi-letter faces and keep single letters scalar", () => {
  for (const letters of ["CH", "SCH", "TSCH", "ÄU", "ÄÖ", "DŽ"]) {
    expect(tileFace.safeParse({ kind: "letters", letters }).success).toBe(true);
  }
  for (const letters of ["", "C", "ch", "Ch", "sCH", "C1", "C H", "C-", "SCHTZ", "ÄÖÜ"]) {
    expect(tileFace.safeParse({ kind: "letters", letters }).success).toBe(false);
  }
  expect(tileFace.safeParse({ kind: "letters" }).success).toBe(false);
  expect(tileFace.safeParse({ kind: "letters", letters: "CH", letter: "C" }).success).toBe(false);
  expect(tileFace.safeParse({ kind: "letter", letter: "CH" }).success).toBe(false);
  expect(tileFace.safeParse({ kind: "letter", letter: "A" }).success).toBe(true);
  expect(tileFace.safeParse({ kind: "blank" }).success).toBe(true);
  expect(tileFace.safeParse({ kind: "blank", letters: "CH" }).success).toBe(false);
});

test("rulesets and the public catalog carry CH and SCH beside C, H and S but no duplicate face", () => {
  expect(ruleset.safeParse(rawRules).success).toBe(true);
  const duplicate = {
    ...rawRules,
    tiles: [...sampleTiles, { face: face("CH"), count: 1, value: 4 }],
  };
  expect(ruleset.safeParse(duplicate).success).toBe(false);
  const catalog = {
    version: 1,
    wordLists: [{ identity: { name: "words", revision: "1" }, label: "Words" }],
    tileProfiles: [
      { identity: { name: "tiles", revision: "1" }, label: "Tiles", tiles: sampleTiles },
    ],
    boardProfiles: [
      {
        identity: { name: "board", revision: "1" },
        label: "Board",
        boardSize: data.ruleset.board_size,
        premiums: data.ruleset.premiums,
      },
    ],
    defaults: {
      selection: {
        wordList: { name: "words", revision: "1" },
        tile: { name: "tiles", revision: "1" },
        board: { name: "board", revision: "1" },
      },
      rules: {
        rack_size: 7,
        bingo_bonus: 50,
        exchange_minimum_bag: 7,
        scoreless_turn_limit: 6,
        minimum_players: 2,
        maximum_players: 4,
      },
      legacyCompatible: false,
    },
  };
  expect(profileCatalog.safeParse(catalog).success).toBe(true);
});

test("snapshots carry whole faces on board, preview, history and the private rack", () => {
  const preview = { player_id: "1", turn: "0", tiles: [boardTile("SCH", 6, 7)] };
  const accepted = {
    ...rawSnapshot({
      board: [boardTile("CH", 7, 7), boardTile("A", 7, 8), boardTile("T", 7, 9)],
      history: committed(["CHAT", "SCH".repeat(15)]),
    }),
    own_preview: preview,
  };
  expect(playerSnapshot.safeParse(accepted).success).toBe(true);
  expect(publicSnapshot.safeParse(accepted.public).success).toBe(true);
  // A spelled word may be longer than 15 characters, but never longer than the board's 15
  // cells times the longest configured face (SCH: 45). Single-letter rulesets keep 15.
  const tooLong = { ...accepted.public, history: committed(["SCH".repeat(15) + "A"]) };
  expect(publicSnapshot.safeParse(tooLong).success).toBe(false);
  for (const invalid of [
    rawSnapshot({ board: [boardTile("ch", 7, 7)] }),
    rawSnapshot({ board: [boardTile("SCHTZ", 7, 7)] }),
    rawSnapshot({ board: [boardTile("", 7, 7)] }),
    // Blank assignments stay single-letter.
    rawSnapshot({ board: [boardTile("CH", 7, 7, true)] }),
    { ...rawSnapshot(), own_preview: { ...preview, tiles: [boardTile("Sch", 6, 7)] } },
    // Proposed placements are private (Decision 2.6); a public preview slot is rejected.
    rawSnapshot({ preview }),
  ]) {
    expect(playerSnapshot.safeParse(invalid).success).toBe(false);
  }
  const lowercaseRack = rawSnapshot();
  lowercaseRack.own_rack.tiles = [{ id: "10", face: { kind: "letters", letters: "ch" } }];
  expect(playerSnapshot.safeParse(lowercaseRack).success).toBe(false);
});

test("rack and shared board render one tile per face with its single value", () => {
  const snapshot = playerSnapshot.parse(
    rawSnapshot({
      board: [
        boardTile("SCH", 6, 7),
        boardTile("CH", 7, 7),
        boardTile("A", 7, 8),
        boardTile("T", 7, 9),
      ],
    }),
  );
  const rules = ruleset.parse(rawRules);
  const rack = renderToStaticMarkup(
    <PlayerRack rack={snapshot.own_rack} rules={rules} canAct={true} />,
  );
  expect([...rack.matchAll(/data-tile-id=/g)]).toHaveLength(7);
  expect(rack).toContain('aria-label="CH, 5 points"');
  expect(rack).toContain('aria-label="SCH, 8 points"');
  expect(rack).toContain('aria-label="Blank tile, 0 points"');
  expect(rack).toMatch(/>CH</);
  expect(rack).toMatch(/>SCH</);
  const board = renderToStaticMarkup(<SharedBoard snapshot={snapshot.public} rules={rules} />);
  expect(board).toContain("Row 8, column 8: Committed CH, 5 points");
  expect(board).toContain("Row 7, column 8: Committed SCH, 8 points");
  expect(board).not.toContain("Tentative");
  expect(board).toMatch(/>CH</);
  expect(board).toMatch(/>SCH</);
  expect(board).not.toContain("Committed CH, ? points");
});

test("drafts place a multi-letter tile on one square and never assign CH or SCH to a blank", () => {
  const snapshot = playerSnapshot.parse(rawSnapshot());
  const rules = ruleset.parse(rawRules);
  const place = (
    draft: ReturnType<typeof emptyDraft>,
    id: string,
    column: number,
    as: string | null,
  ) =>
    editDraft(snapshot, rules, draft, {
      kind: "place",
      tileId: id,
      coordinate: { row: 7, column },
      blankAs: as,
    });
  const lone = place(emptyDraft(snapshot), "10", 7, null);
  expect(lone.problem).toBeUndefined();
  // One CH tile is one cell: not a two-letter word, exactly as the engine counts cells.
  expect(draftProblem(snapshot, rules, lone.draft)).toBe("no-word");
  const pair = place(lone.draft, "13", 8, null);
  expect(pair.problem).toBeUndefined();
  expect(pair.draft.placements).toHaveLength(2);
  expect(draftProblem(snapshot, rules, pair.draft)).toBeUndefined();
  expect(place(lone.draft, "11", 8, "S").problem).toBe("blank");
  for (const multi of ["CH", "SCH"]) {
    expect(place(lone.draft, "12", 8, multi).problem).toBe("blank");
  }
  expect(place(lone.draft, "12", 8, "C").problem).toBeUndefined();
  expect(place(lone.draft, "12", 8, "H").problem).toBeUndefined();
  // B is on no face at all.
  expect(place(lone.draft, "12", 8, "B").problem).toBe("blank");
});
