import { type CommandEnvelope } from "./game-protocol";
import { type PlayerSnapshot, type Ruleset } from "./public-state";

type Placement = Extract<CommandEnvelope["command"], { kind: "preview" }>["placements"][number];
export type MoveDraft = { context: string; placements: Placement[] };
export type DraftAction =
  | { kind: "place"; tileId: string; coordinate: Placement["coordinate"]; blankAs: string | null }
  | { kind: "remove"; tileId: string }
  | { kind: "reset" };
export type DraftProblem =
  | "changed"
  | "waiting"
  | "empty"
  | "too-many"
  | "not-owned"
  | "duplicate"
  | "coordinate"
  | "occupied"
  | "blank"
  | "not-line"
  | "gap"
  | "center"
  | "no-word"
  | "disconnected";
export const draftMessages: Record<DraftProblem, string> = {
  changed: "The game changed. Start a fresh draft.",
  waiting: "Wait for your turn.",
  empty: "Select a tile and a board square.",
  "too-many": "Use no more tiles than your rack holds.",
  "not-owned": "This tile is no longer in your rack.",
  duplicate: "Use each tile and square only once.",
  coordinate: "Choose a square inside the board.",
  occupied: "That square already has a committed tile.",
  blank: "Choose a valid letter for the blank.",
  "not-line": "Place all new tiles in one row or column.",
  gap: "Fill the gaps between your tiles.",
  center: "The first move must cover the center square.",
  "no-word": "A move must form a word of at least two letters.",
  disconnected: "Connect your move to a committed tile.",
};
function key(coordinate: Placement["coordinate"]): string {
  return `${coordinate.row},${coordinate.column}`;
}
function context(snapshot: PlayerSnapshot): string {
  return JSON.stringify({
    game: snapshot.public.game_id,
    player: snapshot.own_rack.player_id,
    phase: snapshot.public.phase,
    rack: snapshot.own_rack.tiles,
    board: snapshot.public.board,
  });
}
export function emptyDraft(snapshot: PlayerSnapshot): MoveDraft {
  return { context: context(snapshot), placements: [] };
}
export function reconcileDraft(snapshot: PlayerSnapshot, draft: MoveDraft): MoveDraft {
  return draft.context === context(snapshot) ? draft : emptyDraft(snapshot);
}
function active(snapshot: PlayerSnapshot): boolean {
  return (
    snapshot.public.phase.kind === "playing" &&
    snapshot.public.phase.active_player === snapshot.own_rack.player_id
  );
}
function placementProblem(
  snapshot: PlayerSnapshot,
  rules: Ruleset,
  placements: Placement[],
): DraftProblem | undefined {
  const tiles = new Set<string>();
  const squares = new Set<string>();
  const committed = new Set(snapshot.public.board.map((tile) => key(tile.coordinate)));
  const letters = new Set(
    rules.tiles.flatMap((tile) => (tile.face.kind === "letter" ? [tile.face.letter] : [])),
  );
  for (const placement of placements) {
    if (tiles.has(placement.tile_id)) {
      return "duplicate";
    }
    const { row, column } = placement.coordinate;
    if (
      !Number.isInteger(row) ||
      !Number.isInteger(column) ||
      row < 0 ||
      column < 0 ||
      row >= rules.board_size ||
      column >= rules.board_size
    ) {
      return "coordinate";
    }
    const square = key(placement.coordinate);
    if (squares.has(square)) {
      return "duplicate";
    }
    if (committed.has(square)) {
      return "occupied";
    }
    const tile = snapshot.own_rack.tiles.find((tile) => tile.id === placement.tile_id);
    if (tile === undefined) {
      return "not-owned";
    }
    if (
      tile.face.kind === "blank"
        ? placement.blank_as === null || !letters.has(placement.blank_as)
        : placement.blank_as !== null
    ) {
      return "blank";
    }
    tiles.add(tile.id);
    squares.add(square);
  }
  return undefined;
}
/** Editing never removes an authoritative rack tile or changes committed board data. */
export function editDraft(
  snapshot: PlayerSnapshot,
  rules: Ruleset,
  draft: MoveDraft,
  action: DraftAction,
): { draft: MoveDraft; problem?: DraftProblem } {
  const current = reconcileDraft(snapshot, draft);
  if (action.kind === "reset") {
    return { draft: emptyDraft(snapshot) };
  }
  if (current !== draft) {
    return { draft: current, problem: "changed" };
  }
  if (!active(snapshot)) {
    return { draft: current, problem: "waiting" };
  }

  const remaining = current.placements.filter((tile) => tile.tile_id !== action.tileId);
  if (action.kind === "remove") {
    return { draft: { ...current, placements: remaining } };
  }
  const placements = [
    ...remaining,
    { tile_id: action.tileId, coordinate: { ...action.coordinate }, blank_as: action.blankAs },
  ];
  const problem = placementProblem(snapshot, rules, placements);
  if (problem !== undefined) {
    return { draft: current, problem };
  }
  return { draft: { ...current, placements } };
}
/** Local structural guidance; authoritative acceptance and dictionary checks remain in Rust. */
export function draftProblem(
  snapshot: PlayerSnapshot,
  rules: Ruleset,
  draft: MoveDraft,
): DraftProblem | undefined {
  if (!active(snapshot)) {
    return "waiting";
  }
  const placements = reconcileDraft(snapshot, draft).placements;
  if (placements.length === 0) {
    return "empty";
  }
  if (placements.length > rules.rack_size) {
    return "too-many";
  }
  const invalid = placementProblem(snapshot, rules, placements);
  if (invalid !== undefined) {
    return invalid;
  }
  const first = placements[0];
  if (first === undefined) {
    return "empty";
  }
  const horizontal = placements.every((tile) => tile.coordinate.row === first.coordinate.row);
  const vertical = placements.every((tile) => tile.coordinate.column === first.coordinate.column);
  if (!horizontal && !vertical) {
    return "not-line";
  }
  const proposed = new Set(placements.map((tile) => key(tile.coordinate)));
  const committed = new Set(snapshot.public.board.map((tile) => key(tile.coordinate)));
  const at = (row: number, column: number) =>
    proposed.has(`${row},${column}`) || committed.has(`${row},${column}`);
  function word(row: number, column: number, dr: number, dc: number): string[] {
    while (row >= dr && column >= dc && at(row - dr, column - dc)) {
      row -= dr;
      column -= dc;
    }
    const result: string[] = [];
    while (row < rules.board_size && column < rules.board_size && at(row, column)) {
      result.push(`${row},${column}`);
      row += dr;
      column += dc;
    }
    return result;
  }
  const { row, column } = first.coordinate;
  const useHorizontal = placements.length === 1 ? word(row, column, 0, 1).length > 1 : horizontal;
  const main = word(row, column, useHorizontal ? 0 : 1, useHorizontal ? 1 : 0);
  if (main.filter((square) => proposed.has(square)).length !== placements.length) {
    return "gap";
  }
  if (
    committed.size === 0 &&
    !proposed.has(`${Math.floor(rules.board_size / 2)},${Math.floor(rules.board_size / 2)}`)
  ) {
    return "center";
  }
  const words = [
    main,
    ...placements.map((tile) =>
      word(
        tile.coordinate.row,
        tile.coordinate.column,
        useHorizontal ? 1 : 0,
        useHorizontal ? 0 : 1,
      ),
    ),
  ].filter((word) => word.length > 1);
  if (words.length === 0) {
    return "no-word";
  }
  if (committed.size > 0 && !words.some((word) => word.some((square) => committed.has(square)))) {
    return "disconnected";
  }
  return undefined;
}
