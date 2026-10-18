import { expect, test } from "bun:test";

import { z } from "zod";

import { placement } from "./game-protocol";
import { draftMessages, draftProblem, emptyDraft } from "./move-draft";
import { playerSnapshot, ruleset } from "./public-state";
import { estimateScore } from "./score-estimate";

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
              score: z.int().min(0).nullable(),
            }),
          ),
        }),
      ),
    })
    .parse(JSON.parse(new TextDecoder().decode(result.stdout)));
  let checked = 0;
  let scored = 0;
  for (const group of oracle.groups) {
    for (const entry of group.cases) {
      const actual = draftProblem(group.snapshot, group.ruleset, {
        ...emptyDraft(group.snapshot),
        placements: entry.placements,
      });
      expect<string | null>(actual ?? null).toBe(entry.expected);
      // The local pre-move estimate equals the native score for every valid move.
      const estimate = estimateScore(group.snapshot, group.ruleset, {
        ...emptyDraft(group.snapshot),
        placements: entry.placements,
      });
      if (entry.score !== null) {
        expect(estimate?.total).toBe(entry.score);
        scored++;
      }
      checked++;
    }
  }
  expect(checked).toBe(1970);
  expect(scored).toBeGreaterThan(0);
}, 60_000);
