import { expect, test } from "bun:test";

import {
  decodeExchangeRejection,
  decodeWordRejection,
  matchesTurnRejection,
  type SubmittedTurnAction,
} from "./game-protocol";

test("private rejection settles only the submitted kind, sequence, match and turn", () => {
  const encode = (error: unknown) =>
    new TextEncoder().encode(
      JSON.stringify({ version: 1, game_id: "1", expected_turn: "4", error }),
    );
  const exchange = decodeExchangeRejection(encode({ kind: "exchange_unavailable" }));
  expect(exchange).toBeDefined();
  if (exchange === undefined) {
    throw new Error("valid native exchange feedback must decode");
  }
  const words = decodeWordRejection(encode({ kind: "invalid_words", words: ["ZZ"] }));
  for (const [kind, feedback] of [
    ["commit", words],
    ["exchange", exchange],
  ] as const) {
    const submitted: SubmittedTurnAction = { kind, sequence: 7, gameId: "1", turn: "4" };
    expect(matchesTurnRejection(submitted, 7, feedback)).toBe(true);
    expect(matchesTurnRejection(undefined, 7, feedback)).toBe(false);
    // Older responses cannot settle a later retry in the same canonical turn.
    expect(matchesTurnRejection({ ...submitted, sequence: 8 }, 7, feedback)).toBe(false);
    expect(matchesTurnRejection({ ...submitted, turn: "5" }, 7, feedback)).toBe(false);
    expect(matchesTurnRejection({ ...submitted, gameId: "2" }, 7, feedback)).toBe(false);
    for (const otherKind of ["commit", "pass", "exchange"] as const) {
      if (otherKind !== kind) {
        expect(matchesTurnRejection({ ...submitted, kind: otherKind }, 7, feedback)).toBe(false);
      }
    }
  }
});
