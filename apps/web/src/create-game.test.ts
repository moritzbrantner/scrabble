import { expect, test } from "bun:test";
import {
  CreationError,
  decodeJoinInformation,
  newCreateRequest,
  type JoinInformation,
} from "./create-game";
const matchId = `g_000000006ac18000_${"01".repeat(16)}`;
const endpoint = "https://server.example/game";
const join = {
  version: 1,
  matchId,
  gameId: "18446744073709551615",
  matchPath: `/game/matches/${matchId}`,
  expiresAt: 1791090000,
} satisfies JoinInformation;

test("creation accepts only public join information bound to its canonical route", () => {
  expect(decodeJoinInformation(join, endpoint)).toEqual(join);
  for (const invalid of [
    { ...join, reconnectToken: "private" },
    { ...join, rack: ["Q"] },
    { ...join, version: 2 },
    { ...join, gameId: 123 },
    { ...join, matchPath: "/game/matches/another-game" },
    { ...join, matchId: "../other" },
  ]) {
    expect(() => decodeJoinInformation(invalid, endpoint)).toThrow(CreationError);
  }
});
test("a new intent produces a fresh 128-bit browser nonce and a safe Unix timestamp", () => {
  const first = newCreateRequest();
  const second = newCreateRequest();
  expect(first.requestId).toMatch(/^[0-9a-f]{32}$/);
  expect(first.requestId).not.toBe(second.requestId);
  expect(Number.isSafeInteger(first.requestedAt)).toBe(true);
  expect(first.version).toBe(2);
});
