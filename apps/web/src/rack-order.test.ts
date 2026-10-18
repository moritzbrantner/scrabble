import { expect, test } from "bun:test";

import { arrangeRack, moveInOrder } from "./rack-order";

const byId = (left: unknown, right: unknown) => String(left).localeCompare(String(right));

const tiles = ["1", "2", "3", "4"].map((id) => ({ id }));

test("arrangement keeps every tile exactly once and appends refills", () => {
  expect(arrangeRack(tiles, ["3", "1"]).map((tile) => tile.id)).toEqual(["3", "1", "2", "4"]);
  // Stale, duplicate and foreign IDs never create or duplicate tiles.
  expect(arrangeRack(tiles, ["9", "2", "2", "4"]).map((tile) => tile.id)).toEqual([
    "2",
    "4",
    "1",
    "3",
  ]);
  expect(arrangeRack(tiles.slice(0, 2), ["4", "2", "1"]).map((tile) => tile.id)).toEqual([
    "2",
    "1",
  ]);
});

test("moving takes the target position in either direction and clamps", () => {
  const order = ["1", "2", "3", "4"];
  expect(moveInOrder(order, "1", 2)).toEqual(["2", "3", "1", "4"]);
  expect(moveInOrder(order, "4", 1)).toEqual(["1", "4", "2", "3"]);
  expect(moveInOrder(order, "2", -5)).toEqual(["2", "1", "3", "4"]);
  expect(moveInOrder(order, "2", 99)).toEqual(["1", "3", "4", "2"]);
  expect(moveInOrder(order, "9", 0)).toEqual(order);
  for (let index = 0; index < 4; index += 1) {
    expect([...moveInOrder(order, "3", index)].sort(byId)).toEqual(order);
  }
});
