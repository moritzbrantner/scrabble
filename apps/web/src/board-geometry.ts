import { type CSSProperties } from "react";

export type BoardGeometry = { board_size: number; board_shape?: "square" | "hexagon" | undefined };
export function containsCell(rules: BoardGeometry, row: number, column: number): boolean {
  const radius = Math.floor(rules.board_size / 2);
  return (
    row >= 0 &&
    column >= 0 &&
    row < rules.board_size &&
    column < rules.board_size &&
    (rules.board_shape !== "hexagon" || (row + column >= radius && row + column <= 3 * radius))
  );
}
/** Presentation coordinates only; legality, words and scores remain Rust-owned. */
export function boardCells(rules: BoardGeometry) {
  const cells: { row: number; column: number; premiumIndex: number }[] = [];
  for (let row = 0; row < rules.board_size; row += 1) {
    for (let column = 0; column < rules.board_size; column += 1) {
      if (containsCell(rules, row, column)) {
        cells.push({ row, column, premiumIndex: cells.length });
      }
    }
  }
  return cells;
}
export function hexCellStyle(size: number, row: number, column: number): CSSProperties {
  const radius = Math.floor(size / 2);
  return {
    left: `${((column + row / 2 - radius / 2) / size) * 100}%`,
    top: `${(row / (size * 0.75 + 0.25)) * 75}%`,
    width: `${100 / size}%`,
    height: `${100 / (size * 0.75 + 0.25)}%`,
  };
}
export function hexBoardStyle(size: number): CSSProperties {
  return {
    aspectRatio: `${size * 44} / ${(size * 0.75 + 0.25) * 51}`,
    width: `${size * 44}px`,
    maxWidth: "100%",
  };
}
