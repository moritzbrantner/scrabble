import { expect, test } from "bun:test";

import data from "./fixtures/private-rack.json";
import {
  draftProblem,
  editDraft,
  emptyDraft,
  persistDraft,
  premoveKey,
  reconcile,
  reconcileDraft,
  restoreDraft,
  type MoveDraft,
} from "./move-draft";
import { playerSnapshot, ruleset, type PlayerSnapshot } from "./public-state";
const snapshot = playerSnapshot.parse(data.snapshots.playing);
const rules = ruleset.parse(data.ruleset);
const tiles = snapshot.own_rack.tiles.filter((tile) => tile.face.kind === "letter");
function put(draft: MoveDraft, index: number, row: number, column: number): MoveDraft {
  const tile = tiles[index];
  if (tile === undefined) {
    throw new Error("Missing test tile");
  }
  const result = editDraft(snapshot, rules, draft, {
    kind: "place",
    tileId: tile.id,
    coordinate: { row, column },
    blankAs: null,
  });
  expect(result.problem).toBeUndefined();
  return result.draft;
}

test("horizontal and vertical drafts move and remove tiles without changing authority", () => {
  const before = JSON.stringify(snapshot);
  for (const vertical of [false, true]) {
    let draft = put(emptyDraft(snapshot), 0, 7, 7);
    expect(draftProblem(snapshot, rules, draft)).toBe("no-word");
    draft = put(draft, 1, vertical ? 8 : 7, vertical ? 7 : 8);
    expect(draftProblem(snapshot, rules, draft)).toBeUndefined();
    draft = put(draft, 1, vertical ? 9 : 7, vertical ? 7 : 9);
    expect(draft.placements).toHaveLength(2);
    expect(draftProblem(snapshot, rules, draft)).toBe("gap");
    draft = editDraft(snapshot, rules, draft, { kind: "remove", tileId: tiles[1]?.id ?? "" }).draft;
    expect(draft.placements).toHaveLength(1);
    expect(editDraft(snapshot, rules, draft, { kind: "reset" }).draft.placements).toEqual([]);
  }
  expect(JSON.stringify(snapshot)).toBe(before);
});
test("invalid edits fail with feedback and retain the previous draft", () => {
  const draft = put(emptyDraft(snapshot), 0, 7, 7);
  for (const [tileId, row, column, blankAs, expected] of [
    ["999999", 7, 8, null, "not-owned"],
    [tiles[1]?.id ?? "", 7, 7, null, "duplicate"],
    [tiles[1]?.id ?? "", 15, 8, null, "coordinate"],
    [tiles[1]?.id ?? "", 7, 8, "A", "blank"],
  ] as const) {
    const result = editDraft(snapshot, rules, draft, {
      kind: "place",
      tileId,
      coordinate: { row, column },
      blankAs,
    });
    expect(result.problem).toBe(expected);
    expect(result.draft).toBe(draft);
  }
  const blank = snapshot.own_rack.tiles.find((tile) => tile.face.kind === "blank");
  expect(blank).toBeDefined();
  const missing = editDraft(snapshot, rules, draft, {
    kind: "place",
    tileId: blank?.id ?? "",
    coordinate: { row: 7, column: 8 },
    blankAs: null,
  });
  expect(missing.problem).toBe("blank");
  const chosen = editDraft(snapshot, rules, draft, {
    kind: "place",
    tileId: blank?.id ?? "",
    coordinate: { row: 7, column: 8 },
    blankAs: "A",
  });
  expect(chosen.problem).toBeUndefined();
  expect(draftProblem(snapshot, rules, chosen.draft)).toBeUndefined();
});
test("structural feedback distinguishes line, gap, center and disconnected moves", () => {
  expect(draftProblem(snapshot, rules, put(put(emptyDraft(snapshot), 0, 7, 7), 1, 8, 8))).toBe(
    "not-line",
  );
  expect(draftProblem(snapshot, rules, put(put(emptyDraft(snapshot), 0, 0, 0), 1, 0, 1))).toBe(
    "center",
  );
  const existing: PlayerSnapshot = {
    ...snapshot,
    public: {
      ...snapshot.public,
      board: [{ coordinate: { row: 7, column: 7 }, letter: "A", is_blank: false }],
    },
  };
  const result = editDraft(existing, rules, emptyDraft(existing), {
    kind: "place",
    tileId: tiles[0]?.id ?? "",
    coordinate: { row: 7, column: 8 },
    blankAs: null,
  });
  expect(draftProblem(existing, rules, result.draft)).toBeUndefined();
  const occupied = editDraft(existing, rules, result.draft, {
    kind: "place",
    tileId: tiles[1]?.id ?? "",
    coordinate: { row: 7, column: 7 },
    blankAs: null,
  });
  expect(occupied.problem).toBe("occupied");
  let isolated = emptyDraft(existing);
  for (const [index, column] of [0, 1].entries()) {
    isolated = editDraft(existing, rules, isolated, {
      kind: "place",
      tileId: tiles[index]?.id ?? "",
      coordinate: { row: 0, column },
      blankAs: null,
    }).draft;
  }
  expect(draftProblem(existing, rules, isolated)).toBe("disconnected");
});
test("authoritative changes reset drafts while presence and previews preserve them", () => {
  const draft = put(emptyDraft(snapshot), 0, 7, 7);
  const presentation: PlayerSnapshot = {
    ...snapshot,
    public: {
      ...snapshot.public,
      players: snapshot.public.players.map((player) => ({ ...player, connected: false })),
    },
    own_preview: { player_id: "1", turn: "0", tiles: [] },
  };
  expect(reconcileDraft(presentation, draft)).toBe(draft);
  for (const next of [
    playerSnapshot.parse(data.snapshots.drawn),
    {
      ...snapshot,
      public: { ...snapshot.public, phase: { kind: "playing", active_player: "2", turn: "1" } },
    } satisfies PlayerSnapshot,
    { ...snapshot, public: { ...snapshot.public, game_id: "2" } },
  ]) {
    expect(reconcileDraft(next, draft).placements).toEqual([]);
    const result = editDraft(next, rules, draft, {
      kind: "place",
      tileId: tiles[1]?.id ?? "",
      coordinate: { row: 7, column: 8 },
      blankAs: null,
    });
    expect(result.problem).toBe("changed");
    expect(result.draft.placements).toEqual([]);
  }
  const finished: PlayerSnapshot = {
    ...snapshot,
    public: { ...snapshot.public, phase: { kind: "finished", winners: ["1"] } },
  };
  const result = editDraft(finished, rules, emptyDraft(finished), {
    kind: "place",
    tileId: tiles[0]?.id ?? "",
    coordinate: { row: 7, column: 7 },
    blankAs: null,
  });
  expect(result.problem).toBe("waiting");
  expect(result.draft.placements).toEqual([]);
});

const waitingAt = (turn: string, board = snapshot.public.board): PlayerSnapshot => ({
  ...snapshot,
  public: {
    ...snapshot.public,
    board,
    phase: { kind: "playing", active_player: turn === "2" ? "1" : "2", turn },
  },
});
const premove = (at: PlayerSnapshot, row: number, columns: number[]) =>
  columns.reduce((draft, column, index) => {
    const result = editDraft(at, rules, draft, {
      kind: "place",
      tileId: tiles[index]?.id ?? "",
      coordinate: { row, column },
      blankAs: null,
    });
    expect(result.problem).toBeUndefined();
    return result.draft;
  }, emptyDraft(at));

test("waiting players keep one compatible private pre-move but never submit it", () => {
  const waiting = waitingAt("1");
  const draft = premove(waiting, 9, [7, 8]);
  expect(draft.placements).toHaveLength(2);
  expect(draftProblem(waiting, rules, draft)).toBe("waiting");
  // A non-overlapping accepted opponent move keeps the whole draft for explicit review.
  const own = waitingAt("2", [{ coordinate: { row: 7, column: 7 }, letter: "A", is_blank: false }]);
  const kept = reconcile(own, draft);
  expect(kept.reset).toBeUndefined();
  expect(kept.draft.placements).toEqual(draft.placements);
  expect(kept.draft.own).toBe(true);
  // The end of the player's own turn (commit, pass, exchange or timeout) ends it.
  expect(reconcile(waitingAt("3"), kept.draft)).toMatchObject({
    reset: "changed",
    draft: { placements: [] },
  });
});

test("an accepted move on any drafted square clears the entire pre-move", () => {
  const draft = premove(waitingAt("1"), 9, [7, 8, 9]);
  const overlapping = waitingAt("2", [
    { coordinate: { row: 9, column: 9 }, letter: "A", is_blank: false },
  ]);
  expect(reconcile(overlapping, draft)).toMatchObject({
    reset: "overlap",
    draft: { placements: [] },
  });
  // A tile that left the rack is stale, never partially kept.
  const lost: PlayerSnapshot = {
    ...waitingAt("2"),
    own_rack: { ...snapshot.own_rack, tiles: snapshot.own_rack.tiles.slice(1) },
  };
  const first = draft.placements[0]?.tile_id;
  expect(lost.own_rack.tiles.some((tile) => tile.id === first)).toBe(
    snapshot.own_rack.tiles[0]?.id !== first,
  );
});

test("same-device persistence restores only a compatible draft for the same match and player", () => {
  const store = new Map<string, string>();
  globalThis.localStorage = {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
  } as Storage;
  const waiting = waitingAt("1");
  const key = premoveKey(waiting);
  const draft = premove(waiting, 9, [7, 8]);
  persistDraft(key, draft);
  expect(restoreDraft(key, waiting, rules)?.placements).toEqual(draft.placements);
  expect(restoreDraft(key, waitingAt("2"), rules)?.placements).toEqual(draft.placements);
  // A foreign match/player binding or corrupt record is discarded.
  const foreign: PlayerSnapshot = { ...waiting, public: { ...waiting.public, game_id: "9" } };
  expect(restoreDraft(key, foreign, rules)).toBeUndefined();
  expect(store.has(key)).toBe(false);
  store.set(key, "{not json");
  expect(restoreDraft(key, waiting, rules)).toBeUndefined();
  persistDraft(key, draft);
  const overlap = waitingAt("2", [
    { coordinate: { row: 9, column: 7 }, letter: "A", is_blank: false },
  ]);
  expect(restoreDraft(key, overlap, rules)).toBeUndefined();
  expect(store.has(key)).toBe(false);
  persistDraft(key, emptyDraft(waiting));
  expect(store.has(key)).toBe(false);
});
