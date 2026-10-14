import { z } from "zod";
import { ruleset } from "./public-state";

export const profileSelection = z.strictObject({
  wordList: ruleset.shape.dictionary,
  tile: ruleset.shape.identity,
  board: ruleset.shape.identity,
});
export const matchSettings = z
  .strictObject({
    rack_size: ruleset.shape.rack_size,
    bingo_bonus: ruleset.shape.bingo_bonus,
    exchange_minimum_bag: ruleset.shape.exchange_minimum_bag,
    scoreless_turn_limit: ruleset.shape.scoreless_turn_limit,
    minimum_players: ruleset.shape.minimum_players,
    maximum_players: ruleset.shape.maximum_players,
  })
  .refine((value) => value.minimum_players <= value.maximum_players);
export type MatchSettings = z.infer<typeof matchSettings>;
export const profileCatalog = z.strictObject({
  version: z.literal(1),
  wordLists: z
    .array(z.strictObject({ identity: ruleset.shape.dictionary, label: z.string() }))
    .min(1)
    .max(32),
  tileProfiles: z
    .array(
      z.strictObject({
        identity: ruleset.shape.identity,
        label: z.string(),
        tiles: ruleset.shape.tiles,
      }),
    )
    .min(1)
    .max(32),
  boardProfiles: z
    .array(
      z.strictObject({
        identity: ruleset.shape.identity,
        label: z.string(),
        boardSize: z.int().min(1).max(15),
        premiums: ruleset.shape.premiums,
      }),
    )
    .min(1)
    .max(32),
  defaults: z.strictObject({
    selection: profileSelection,
    rules: matchSettings,
    legacyCompatible: z.boolean(),
  }),
});
export type ProfileCatalog = z.infer<typeof profileCatalog>;
