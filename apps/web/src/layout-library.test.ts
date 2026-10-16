import { expect, test } from "bun:test";

import { cells, reshape, validSize } from "./layout-library";

test("editor geometry keeps complete regular grids and re-centres premiums on resize", () => {
  expect(cells("square", 9)).toHaveLength(81);
  expect(cells("hexagon", 7)).toHaveLength(37);
  expect(cells("hexagon", 15)).toHaveLength(169);
  expect([8, 1, 17].map((size) => validSize(size, "hexagon"))).toEqual([false, false, false]);
  expect([1, 3, 7, 15].map((size) => validSize(size, "square"))).toEqual([true, true, true, true]);
  expect([3, 7, 15].map((size) => validSize(size, "hexagon"))).toEqual([true, true, true]);
  const hex7 = cells("hexagon", 7).map((cell) =>
    cell.row === 3 && cell.column === 3 ? "triple_word" : "normal",
  );
  const hex9 = reshape({ shape: "hexagon", size: 7, premiums: hex7 }, "hexagon", 9);
  expect(hex9).toHaveLength(61);
  const center = cells("hexagon", 9).find((cell) => cell.row === 4 && cell.column === 4)!;
  expect(hex9[center.index]).toBe("triple_word");
  expect(hex9.filter((premium) => premium !== "normal")).toHaveLength(1);
  const square = reshape({ shape: "hexagon", size: 9, premiums: hex9 }, "square", 5);
  expect(square).toHaveLength(25);
  expect(square[12]).toBe("triple_word");
});
