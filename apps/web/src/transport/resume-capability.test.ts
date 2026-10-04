import { expect, test } from "bun:test";
import { BrowserMatch } from "./browser-match";
import { resumeCapability, type ResumeCapability } from "./resume-capability";

test("saved player capabilities reject malformed, excessive and cross-match claims", () => {
  const saved = {
    version: 1,
    route: "https://example.test/game/matches/one",
    playerId: "7",
    token: "ab".repeat(16),
    sequence: 19,
  } satisfies ResumeCapability;
  expect(resumeCapability.parse(saved)).toEqual(saved);
  for (const invalid of [
    { ...saved, token: "short" },
    { ...saved, playerId: "4294967296" },
    { ...saved, playerId: "07" },
    { ...saved, sequence: -1 },
    { ...saved, sequence: 0x100000000 },
    { ...saved, own_rack: [] },
    { ...saved, version: 2 },
  ]) {
    expect(resumeCapability.safeParse(invalid).success).toBe(false);
  }
  expect(
    () =>
      new BrowserMatch({
        endpoint: "https://example.test/game",
        matchId: "two",
        resume: saved,
        onState: () => {},
        onSnapshot: () => {},
      }),
  ).toThrow("another match");
});
