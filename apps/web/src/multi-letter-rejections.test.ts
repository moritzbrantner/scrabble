// Regressions for #98 review findings: multi-letter rulesets keep their protocol bounds.
import { expect, test } from "bun:test";

import data from "./fixtures/private-rack.json";
import { decodeWordRejection } from "./game-protocol";
import { maxWordLength, ruleset } from "./public-state";

const encode = (value: unknown) => new TextEncoder().encode(JSON.stringify(value));
const rejection = (words: string[]) =>
  encode({ version: 1, game_id: "1", expected_turn: "0", error: { kind: "invalid_words", words } });
const face = (spelling: string) =>
  Array.from(spelling).length === 1
    ? { kind: "letter", letter: spelling }
    : { kind: "letters", letters: spelling };

test("invalid-word feedback accepts words up to the ruleset's spelled-word bound", () => {
  const rules = ruleset.parse({
    ...data.ruleset,
    tiles: [
      { face: face("A"), count: 30, value: 1 },
      { face: face("SCH"), count: 10, value: 8 },
    ],
  });
  const bound = maxWordLength(rules);
  expect(bound).toBe(45);
  // Six SCH tiles spell an 18-letter word the server reports in full.
  const long = "SCH".repeat(6);
  expect(decodeWordRejection(rejection([long]), bound).error.words).toEqual([long]);
  expect(() => decodeWordRejection(rejection(["SCH".repeat(15) + "A"]), bound)).toThrow("protocol");
  // Single-letter rulesets (and the default) keep the 15-letter limit.
  expect(maxWordLength(ruleset.parse(data.ruleset))).toBe(15);
  expect(() => decodeWordRejection(rejection(["A".repeat(16)]))).toThrow("protocol");
});

test("rulesets need at least one single-letter face, as the engine requires", () => {
  const only = {
    ...data.ruleset,
    tiles: [
      { face: face("CH"), count: 30, value: 5 },
      { face: face("SCH"), count: 10, value: 8 },
      { face: { kind: "blank" }, count: 2, value: 0 },
    ],
  };
  expect(ruleset.safeParse(only).success).toBe(false);
  expect(
    ruleset.safeParse({ ...only, tiles: [...only.tiles, { face: face("A"), count: 1, value: 1 }] })
      .success,
  ).toBe(true);
});
