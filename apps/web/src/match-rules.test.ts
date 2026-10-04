import { describe, expect, test } from "bun:test";
import { fixtures } from "./fixtures";
import { matchRules, requireMatchRules } from "./match-rules";

describe("match rules", () => {
  test("authored and deployment dictionaries share the supported English geometry", () => {
    expect(matchRules(fixtures.snapshots.playing)).toEqual(fixtures.ruleset);
    const snapshot = {
      ...fixtures.snapshots.playing,
      ruleset: { ...fixtures.ruleset.identity, revision: `1+dictionary-sha256:${"a".repeat(64)}` },
      dictionary: { name: "deployment-list", revision: `2026.1+sha256:${"b".repeat(64)}` },
    };
    const rules = matchRules(snapshot);
    expect(rules.identity).toEqual(snapshot.ruleset);
    expect(rules.dictionary).toEqual(snapshot.dictionary);
    expect(rules.tiles).toBe(fixtures.ruleset.tiles);
    expect(rules.premiums).toBe(fixtures.ruleset.premiums);
    expect(() => requireMatchRules(snapshot, rules)).not.toThrow();
    expect(() =>
      requireMatchRules(
        { ...snapshot, dictionary: { ...snapshot.dictionary, name: "changed" } },
        rules,
      ),
    ).toThrow("changed during the connection");
  });
  test("unknown geometry and unbound dictionary identities fail closed", () => {
    for (const [ruleset, dictionary] of [
      [{ ...fixtures.ruleset.identity, name: "unknown-board" }, fixtures.ruleset.dictionary],
      [fixtures.ruleset.identity, { ...fixtures.ruleset.dictionary, revision: "2" }],
      [
        { ...fixtures.ruleset.identity, revision: `2+dictionary-sha256:${"a".repeat(64)}` },
        fixtures.ruleset.dictionary,
      ],
      [
        { ...fixtures.ruleset.identity, revision: `1+dictionary-sha256:${"a".repeat(64)}` },
        { name: "deployment", revision: "1" },
      ],
    ]) {
      if (ruleset === undefined || dictionary === undefined) {
        throw new Error("Missing identity fixture");
      }
      expect(() => matchRules({ ...fixtures.snapshots.playing, ruleset, dictionary })).toThrow(
        "does not support",
      );
    }
  });
});
