import { expect, test } from "bun:test";

import { z } from "zod";

import { placement } from "./game-protocol";
import { draftMessages, draftProblem, emptyDraft } from "./move-draft";
import { playerSnapshot, ruleset } from "./public-state";

// Cargo may compile a different feature graph on a cold CI checkout; allow compilation time.
test("browser structural guidance matches the Rust oracle across empty, committed and smaller boards", () => {
  const result = Bun.spawnSync([
    "cargo",
    "run",
    "-q",
    "-p",
    "scrabble-game",
    "--example",
    "draft_validation_fixtures",
    "--locked",
  ]);
  expect(result.exitCode).toBe(0);
  const oracle = z
    .strictObject({
      groups: z.array(
        z.strictObject({
          ruleset,
          snapshot: playerSnapshot,
          cases: z.array(
            z.strictObject({
              placements: z.array(placement),
              expected: z.enum(Object.keys(draftMessages)).nullable(),
            }),
          ),
        }),
      ),
    })
    .parse(JSON.parse(new TextDecoder().decode(result.stdout)));
  let checked = 0;
  for (const group of oracle.groups) {
    for (const entry of group.cases) {
      const actual = draftProblem(group.snapshot, group.ruleset, {
        ...emptyDraft(group.snapshot),
        placements: entry.placements,
      });
      expect<string | null>(actual ?? null).toBe(entry.expected);
      checked++;
    }
  }
  expect(checked).toBe(1970);
}, 60_000);
