/** Deterministic display-only late-game workload for optimized browser measurements.
 * Its connected geometry and tile inventory are plausible, but it is not
 * an authoritative replay or dictionary-validated match.
 */
import { type PlayerSnapshot, type Ruleset } from "../src/public-state";

export type BrowserWorkload = "opening" | "populated";
export type BoardFixturePhase = "playing" | "finished" | "populated" | "populated-updated";

export const POPULATED_BOARD_TILE_COUNT = 81;
export const POPULATED_DRAFT_SQUARE_INDICES = [118, 119] as const;

// Connected horizontal/vertical runs crossing the center; column 13/14 of row 8
// stay open for an AH draft that extends the committed run.
const runs = [
  [7, 2, 7, 12],
  [1, 7, 13, 7],
  [4, 4, 10, 4],
  [4, 10, 10, 10],
  [4, 4, 4, 10],
  [10, 4, 10, 10],
  [2, 6, 2, 12],
  [12, 6, 12, 12],
  [2, 12, 12, 12],
  [4, 2, 10, 2],
  [6, 2, 6, 12],
  [8, 2, 8, 12],
] as const;

export function populatedPlayerSnapshot(opening: PlayerSnapshot, rules: Ruleset): PlayerSnapshot {
  const indices = new Set<number>();
  for (const [rowStart, columnStart, rowEnd, columnEnd] of runs) {
    const rowStep = Math.sign(rowEnd - rowStart);
    const columnStep = Math.sign(columnEnd - columnStart);
    const steps = Math.max(Math.abs(rowEnd - rowStart), Math.abs(columnEnd - columnStart));
    for (let step = 0; step <= steps; step++) {
      indices.add((rowStart + step * rowStep) * rules.board_size + columnStart + step * columnStep);
    }
  }
  if (
    rules.board_size !== 15 ||
    indices.size !== POPULATED_BOARD_TILE_COUNT ||
    !indices.has(112) ||
    POPULATED_DRAFT_SQUARE_INDICES.some((index) => indices.has(index))
  ) {
    throw new Error("Populated performance geometry changed");
  }

  // Do not duplicate any face already held in the measured player's private rack.
  const stock = rules.tiles.map((tile) => ({ face: tile.face, count: tile.count }));
  for (const tile of opening.own_rack.tiles) {
    const entry = stock.find(({ face }) =>
      face.kind === "blank"
        ? tile.face.kind === "blank"
        : tile.face.kind === "letter" && face.letter === tile.face.letter,
    );
    if (entry === undefined || entry.count === 0) {
      throw new Error("Populated fixture exhausts the authored tile inventory");
    }
    entry.count--;
  }

  // Interleave face types to avoid an artificial board consisting of alphabetic runs.
  const faces: Ruleset["tiles"][number]["face"][] = [];
  while (faces.length < indices.size) {
    const before = faces.length;
    for (const entry of stock) {
      if (entry.count === 0) continue;
      faces.push(entry.face);
      entry.count--;
      if (faces.length === indices.size) break;
    }
    if (faces.length === before) throw new Error("Insufficient tiles for populated fixture");
  }

  const otherRackTiles = opening.public.players
    .filter((player) => player.id !== opening.own_rack.player_id)
    .reduce((sum, player) => sum + player.rack_count, 0);
  const remainingTiles = stock.reduce((sum, entry) => sum + entry.count, 0) - otherRackTiles;
  if (remainingTiles < 1) throw new Error("Populated fixture must retain a nonempty bag");

  const board = [...indices]
    .sort((left, right) => left - right)
    .map((index, slot) => {
      const face = faces[slot]!;
      return {
        coordinate: { row: Math.floor(index / rules.board_size), column: index % rules.board_size },
        letter: face.kind === "blank" ? "E" : face.letter,
        is_blank: face.kind === "blank",
      };
    });

  return {
    ...opening,
    public: {
      ...opening.public,
      revision: "41",
      phase: { kind: "playing", active_player: opening.own_rack.player_id, turn: "38" },
      board,
      remaining_tiles: remainingTiles,
      players: opening.public.players.map((player, index) => ({
        ...player,
        score: index === 0 ? 178 : 153,
      })),
    },
  };
}
