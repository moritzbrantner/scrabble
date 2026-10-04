import { describe, expect, test } from "bun:test";
import { fixtures } from "./fixtures";
import variant from "./fixtures/ruleset-variant.json";
import { playerSnapshot, ruleset } from "./public-state";
import { matchRules, requireMatchRules } from "./match-rules";

describe("match rules", () => {
  test("presentation derives each alphabet, distribution and geometry from its authoritative configuration", () => {
    const german = playerSnapshot.parse(variant.playing).public;
    expect(matchRules(fixtures.snapshots.playing)).toEqual(fixtures.ruleset);
    expect(matchRules(german)).toEqual(ruleset.parse(variant.ruleset));
    expect(matchRules(german).rack_size).toBe(3);
    expect(matchRules(german).board_size).toBe(7);
    expect(matchRules(german).tiles).not.toEqual(fixtures.ruleset.tiles);
    expect(() => requireMatchRules(german, matchRules(german))).not.toThrow();
  });
  test("unbound identities and changes to claimed immutable content fail closed", () => {
    const snapshot = fixtures.snapshots.playing;
    for (const changed of [
      { ...snapshot, ruleset: { ...snapshot.ruleset, name: "another-profile" } },
      { ...snapshot, dictionary: { ...snapshot.dictionary, revision: "2" } },
      { ...snapshot, configuration: { ...snapshot.configuration, rack_size: 0 } },
    ]) {
      expect(() => matchRules(changed)).toThrow("configuration is invalid");
    }
    const changed = {
      ...snapshot,
      configuration: {
        ...snapshot.configuration,
        bingo_bonus: snapshot.configuration.bingo_bonus + 1,
      },
    };
    expect(() => requireMatchRules(changed, matchRules(snapshot))).toThrow(
      "changed during the connection",
    );
  });
});
