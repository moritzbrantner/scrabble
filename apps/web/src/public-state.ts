import { z } from "zod";

export const id = z
  .string()
  .max(20)
  .regex(/^(0|[1-9][0-9]*)$/)
  .pipe(z.string().refine((value) => BigInt(value) <= 18446744073709551615n));
export const letter = z.string().refine((value) => {
  const scalar = value.codePointAt(0);
  return (
    Array.from(value).length === 1 && scalar !== undefined && (scalar < 0xd800 || scalar > 0xdfff)
  );
});
export const coordinate = z.strictObject({
  row: z.int().min(0).max(14),
  column: z.int().min(0).max(14),
});
export const tileFace = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("letter"), letter }),
  z.strictObject({ kind: z.literal("blank") }),
]);
const identity = z.strictObject({ name: z.string().min(1), revision: z.string().min(1) });
const tile = z.strictObject({ coordinate, letter, is_blank: z.boolean() });
export const premium = z.enum([
  "normal",
  "double_letter",
  "triple_letter",
  "double_word",
  "triple_word",
]);
const phase = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("lobby") }),
  z.strictObject({ kind: z.literal("playing"), active_player: id, turn: id }),
  z.strictObject({ kind: z.literal("finished"), winners: z.array(id) }),
]);
export const turnAction = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("commit"),
    words: z
      .array(
        z
          .string()
          .min(1)
          .max(30)
          .refine((word) => Array.from(word).length <= 15),
      )
      .max(16),
    move_score: z.int().min(0).max(4294967295),
    blank_count: z.int().min(0).max(15),
  }),
  z.strictObject({ kind: z.literal("pass") }),
  z.strictObject({ kind: z.literal("exchange"), tile_count: z.int().min(1).max(15) }),
]);
const publicTurn = z.strictObject({
  turn: id,
  player_id: id,
  action: turnAction,
  scores: z
    .array(
      z.strictObject({
        player_id: id,
        delta: z.int().min(-4294967295).max(4294967295),
        score: z.int().min(-2147483648).max(2147483647),
      }),
    )
    .max(4),
});
export const publicSnapshot = z.strictObject({
  version: z.literal(1),
  game_id: id,
  revision: id,
  ruleset: identity,
  dictionary: identity,
  phase,
  board: z.array(tile).max(225),
  players: z
    .array(
      z.strictObject({
        id,
        display_name: z.string(),
        score: z.int().min(-2147483648).max(2147483647),
        rack_count: z.int().min(0).max(15),
        connected: z.boolean().optional(),
      }),
    )
    .max(4),
  remaining_tiles: z.int().min(0).max(65535),
  history: z.array(publicTurn).max(24),
  host: z.strictObject({ id: id.nullable() }).optional(),
  preview: z.strictObject({ player_id: id, turn: id, tiles: z.array(tile).max(15) }).nullable(),
});
export const ruleset = z
  .strictObject({
    identity,
    dictionary: identity,
    tiles: z.array(
      z.strictObject({
        face: tileFace,
        count: z.int().positive(),
        value: z.int().min(0).max(65535),
      }),
    ),
    board_size: z.int().min(1).max(15),
    premiums: z.array(premium),
    rack_size: z.int().min(1).max(15),
    bingo_bonus: z.int(),
    exchange_minimum_bag: z.int(),
    scoreless_turn_limit: z.int(),
    minimum_players: z.int(),
    maximum_players: z.int(),
  })
  .refine((value) => value.premiums.length === value.board_size ** 2);
export type PublicSnapshot = z.infer<typeof publicSnapshot>;
export type Ruleset = z.infer<typeof ruleset>;
export type Premium = z.infer<typeof premium>;

export const playerSnapshot = z.strictObject({
  public: publicSnapshot,
  own_rack: z.strictObject({
    player_id: id,
    tiles: z.array(z.strictObject({ id, face: tileFace })).max(15),
  }),
});
export type PlayerSnapshot = z.infer<typeof playerSnapshot>;
