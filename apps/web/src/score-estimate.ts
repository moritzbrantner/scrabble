/**
 * Non-authoritative local score estimate for a waiting player's private pre-move
 * (Decision 1.54). It mirrors the native word discovery and scoring so the number matches
 * what the server would award, but it never checks the dictionary and never decides legality:
 * the server's private evaluation replaces it on the player's turn, and the commit is
 * always scored and checked by the server.
 */
import { boardCells, containsCell } from "./board-geometry";
import { type MoveDraft } from "./move-draft";
import {
  faceText,
  faceValues,
  type MoveAnalysis,
  type PlayerSnapshot,
  type Ruleset,
} from "./public-state";

type Direction = "horizontal" | "vertical" | "diagonal";
const steps: Record<Direction, [number, number]> = {
  horizontal: [0, 1],
  vertical: [1, 0],
  diagonal: [1, -1],
};
type Cell = { row: number; column: number; letter: string; value: number; isNew: boolean };
const multipliers = {
  normal: [1, 1],
  double_letter: [2, 1],
  triple_letter: [3, 1],
  double_word: [1, 2],
  triple_word: [1, 3],
} as const;

export function estimateScore(
  snapshot: PlayerSnapshot,
  rules: Ruleset,
  draft: MoveDraft,
): MoveAnalysis | undefined {
  const placements = draft.placements;
  const first = placements[0];
  if (first === undefined || placements.length > rules.rack_size) {
    return undefined;
  }
  const values = faceValues(rules);
  const premiumIndex = new Map(
    boardCells(rules).map((cell) => [`${cell.row},${cell.column}`, cell.premiumIndex]),
  );
  const cells = new Map<string, Cell>();
  for (const tile of snapshot.public.board) {
    cells.set(`${tile.coordinate.row},${tile.coordinate.column}`, {
      ...tile.coordinate,
      letter: tile.letter,
      value: tile.is_blank ? 0 : (values.get(tile.letter) ?? 0),
      isNew: false,
    });
  }
  const committedCount = cells.size;
  for (const placement of placements) {
    const tile = snapshot.own_rack.tiles.find((candidate) => candidate.id === placement.tile_id);
    const text = tile === undefined ? null : faceText(tile.face);
    const letter = placement.blank_as ?? text;
    if (tile === undefined || letter === null) {
      return undefined;
    }
    cells.set(`${placement.coordinate.row},${placement.coordinate.column}`, {
      ...placement.coordinate,
      letter,
      value: tile.face.kind === "blank" ? 0 : (values.get(letter) ?? 0),
      isNew: true,
    });
  }
  const hex = rules.board_shape === "hexagon";
  const axes: Direction[] = hex
    ? ["horizontal", "vertical", "diagonal"]
    : ["horizontal", "vertical"];
  const onLine = (direction: Direction) =>
    placements.every(({ coordinate }) => {
      if (direction === "horizontal") {
        return coordinate.row === first.coordinate.row;
      }
      if (direction === "vertical") {
        return coordinate.column === first.coordinate.column;
      }
      return coordinate.row + coordinate.column === first.coordinate.row + first.coordinate.column;
    });
  const line = axes.find(onLine);
  if (line === undefined) {
    return undefined;
  }
  const at = (row: number, column: number) =>
    containsCell(rules, row, column) ? cells.get(`${row},${column}`) : undefined;
  const word = (row: number, column: number, direction: Direction) => {
    const [dr, dc] = steps[direction];
    while (at(row - dr, column - dc) !== undefined) {
      row -= dr;
      column -= dc;
    }
    const tiles: Cell[] = [];
    for (let cell = at(row, column); cell !== undefined; cell = at(row, column)) {
      tiles.push(cell);
      row += dr;
      column += dc;
    }
    return { direction, tiles };
  };
  const { row, column } = first.coordinate;
  const length = (tiles: Cell[]) =>
    rules.main_word_length === "letters"
      ? tiles.reduce((sum, tile) => sum + Array.from(tile.letter).length, 0)
      : tiles.length;
  let direction: Direction = line;
  if (placements.length === 1) {
    if (hex) {
      // Decision 1.15: longest formed word; ties follow the fixed axis order.
      let best = 0;
      direction = "horizontal";
      for (const axis of axes) {
        const candidate = word(row, column, axis).tiles;
        if (candidate.length > 1 && length(candidate) > best) {
          best = length(candidate);
          direction = axis;
        }
      }
    } else {
      direction = word(row, column, "horizontal").tiles.length > 1 ? "horizontal" : "vertical";
    }
  }
  const main = word(row, column, direction);
  if (main.tiles.filter((tile) => tile.isNew).length !== placements.length) {
    return undefined;
  }
  const center = Math.floor(rules.board_size / 2);
  if (committedCount === 0 && !cells.has(`${center},${center}`)) {
    return undefined;
  }
  const words = main.tiles.length > 1 ? [main] : [];
  const proposed = [...cells.values()]
    .filter((cell) => cell.isNew)
    .sort((left, right) => left.row - right.row || left.column - right.column);
  for (const tile of proposed) {
    for (const axis of axes.filter((axis) => axis !== direction)) {
      const cross = word(tile.row, tile.column, axis);
      if (cross.tiles.length > 1) {
        words.push(cross);
      }
    }
  }
  if (
    words.length === 0 ||
    (committedCount > 0 && !words.some((formed) => formed.tiles.some((tile) => !tile.isNew)))
  ) {
    return undefined;
  }
  let total = 0;
  const scored = words.map((formed, index) => {
    const premiums = index === 0 || (rules.premium_policy ?? "all_word") === "all_word";
    let sum = 0;
    let factor = 1;
    for (const tile of formed.tiles) {
      const premium =
        tile.isNew && premiums
          ? (rules.premiums[premiumIndex.get(`${tile.row},${tile.column}`) ?? -1] ?? "normal")
          : "normal";
      const [letter, wordFactor] = multipliers[premium];
      sum += tile.value * letter;
      factor *= wordFactor;
    }
    const score = sum * factor;
    total += score;
    return {
      text: formed.tiles.map((tile) => tile.letter).join(""),
      direction: formed.direction,
      score,
    };
  });
  const bingo = placements.length === rules.rack_size ? rules.bingo_bonus : 0;
  return {
    words: scored,
    length_metric: rules.main_word_length ?? "tiles",
    bingo_bonus: bingo,
    total: total + bingo,
  };
}
