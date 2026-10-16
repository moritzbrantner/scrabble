// Independent #96 wire bound. Native hexagonal.rs proves a real 15-tile move
// can form 31 words; square-only sixteen-word limits cannot decode that history.
import { expect, test } from "bun:test";

import { decodeWordRejection } from "./game-protocol";
import { turnAction } from "./public-state";

test("a legal hex commit can carry thirty-one formed words", () => {
  expect(
    turnAction.safeParse({
      kind: "commit",
      words: Array.from({ length: 31 }, () => "AA"),
      move_score: 62,
      blank_count: 0,
    }).success,
  ).toBe(true);
});

test("formed-word arrays still reject beyond the maximum three-axis move", () => {
  expect(
    turnAction.safeParse({
      kind: "commit",
      words: Array.from({ length: 32 }, () => "AA"),
      move_score: 64,
      blank_count: 0,
    }).success,
  ).toBe(false);
});

const invalidWords = (count: number) =>
  Array.from(
    { length: count },
    (_, index) =>
      `A${String.fromCharCode(65 + Math.floor(index / 26))}${String.fromCharCode(65 + (index % 26))}`,
  );
const rejection = (words: string[]) =>
  new TextEncoder().encode(
    JSON.stringify({
      version: 1,
      game_id: "1",
      expected_turn: "0",
      error: { kind: "invalid_words", words },
    }),
  );
test("hex invalid-word feedback retains a complete bounded three-axis word set", () => {
  const words = invalidWords(31);
  expect(decodeWordRejection(rejection(words)).error.words).toEqual(words);
  expect(() => decodeWordRejection(rejection(invalidWords(32)))).toThrow("protocol");
  expect(() => decodeWordRejection(rejection(["AA", "AA"]))).toThrow("protocol");
});
