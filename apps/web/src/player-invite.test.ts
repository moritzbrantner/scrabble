import { expect, test } from "bun:test";

import { playerInviteUrl, validInviteId } from "./player-invite";
const matchId = `g_${"0".repeat(16)}_${"a".repeat(32)}`;
test("Pages invites rebuild an unambiguous public route without private query state", () => {
  const page = new URL(
    "https://moritzbrantner.github.io/scrabble/?token=secret&requestId=private&server=https://example.test/game&api=https://example.test&fixture=lobby#private",
  );
  const invite = playerInviteUrl(page, matchId, false);
  expect(invite.pathname).toBe("/scrabble/");
  expect(invite.searchParams.get("match")).toBe(matchId);
  expect([...invite.searchParams.keys()]).toEqual(["view", "match"]);
  expect(invite.hash).toBe("");
  expect(new URL(invite.href).searchParams.get("match")).toBe(matchId);
  const local = playerInviteUrl(page, matchId, true);
  expect([...local.searchParams.keys()]).toEqual(["view", "match", "server", "api"]);
  expect(local.href).not.toContain("secret");
  expect(local.href).not.toContain("private");
  for (const value of [
    "",
    "../match",
    `${matchId}/reconnect/secret`,
    matchId.toUpperCase(),
    `${matchId}&match=other`,
  ]) {
    expect(validInviteId(value)).toBe(false);
    expect(() => playerInviteUrl(page, value, false)).toThrow();
  }
  expect(() =>
    playerInviteUrl(
      new URL(
        "https://example.test/scrabble/?server=https://example.test/game&server=https://other.test/game",
      ),
      matchId,
      true,
    ),
  ).toThrow();
});

test("invites include only a supported unambiguous interface language", () => {
  for (const locale of ["en", "de", "es"]) {
    const invite = playerInviteUrl(
      new URL(`https://example.test/scrabble/?lang=${locale}&token=private`),
      matchId,
      false,
    );
    expect(invite.searchParams.get("lang")).toBe(locale);
    expect(invite.searchParams.has("token")).toBe(false);
  }
  for (const query of ["lang=fr", "lang=de&lang=es", "lang=private"]) {
    expect(
      playerInviteUrl(
        new URL(`https://example.test/scrabble/?${query}`),
        matchId,
        false,
      ).searchParams.has("lang"),
    ).toBe(false);
  }
});
