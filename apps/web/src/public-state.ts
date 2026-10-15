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
const utf8 = new TextEncoder();
/** Rust `char::is_alphabetic() && !char::is_lowercase()`, the word-list and face rule. */
const faceCharacter = (character: string) =>
  /^\p{Alphabetic}$/u.test(character) && !/^\p{Lowercase}$/u.test(character);
/** Multi-letter faces (#116): at least two letters, at most 4 UTF-8 bytes. */
export const MAX_FACE_BYTES = 4;
export const faceLetters = z
  .string()
  .refine(
    (value) =>
      Array.from(value).length >= 2 &&
      utf8.encode(value).length <= MAX_FACE_BYTES &&
      Array.from(value).every(faceCharacter),
  );
/** A cell's spelled face: one scalar (single letter or blank assignment) or a whole face. */
const spelledFace = z
  .string()
  .refine((value) => letter.safeParse(value).success || faceLetters.safeParse(value).success);
export const coordinate = z.strictObject({
  row: z.int().min(0).max(14),
  column: z.int().min(0).max(14),
});
export const tileFace = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("letter"), letter }),
  z.strictObject({ kind: z.literal("letters"), letters: faceLetters }),
  z.strictObject({ kind: z.literal("blank") }),
]);
export type TileFace = z.infer<typeof tileFace>;
/** The letters a face spells, or `null` for a blank. */
export function faceText(face: TileFace): string | null {
  switch (face.kind) {
    case "letter": {
      return face.letter;
    }
    case "letters": {
      return face.letters;
    }
    case "blank": {
      return null;
    }
  }
}
/** Configured value of every spelled face (single and multi-letter). */
export function faceValues(rules: { tiles: { face: TileFace; value: number }[] }) {
  return new Map(
    rules.tiles.flatMap((tile) => {
      const text = faceText(tile.face);
      return text === null ? [] : [[text, tile.value] as const];
    }),
  );
}
const identity = z.strictObject({
  name: z
    .string()
    .min(1)
    .max(64)
    .regex(/^[A-Za-z0-9_.-]+$/),
  revision: z
    .string()
    .min(1)
    .max(192)
    .regex(/^[A-Za-z0-9_.+:-]+$/),
});
const tile = z
  .strictObject({ coordinate, letter: spelledFace, is_blank: z.boolean() })
  // Blank assignments stay single-letter (#116).
  .refine((value) => !value.is_blank || letter.safeParse(value.letter).success);
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
export const moveAnalysis = z.strictObject({
  words: z
    .array(
      z.strictObject({
        text: z.string().min(1).max(60),
        direction: z.enum(["horizontal", "vertical", "diagonal"]),
        score: z.int().min(0).max(4294967295),
      }),
    )
    .min(1)
    .max(31),
  length_metric: z.enum(["tiles", "letters"]),
  bingo_bonus: z.int().min(0).max(4294967295),
  total: z.int().min(0).max(4294967295),
});
export type MoveAnalysis = z.infer<typeof moveAnalysis>;
const placementEvaluation = z.discriminatedUnion("status", [
  z.strictObject({ status: z.literal("valid"), analysis: moveAnalysis }),
  z.strictObject({
    status: z.literal("invalid"),
    problem: z.enum([
      "center",
      "not_line",
      "gap",
      "disconnected",
      "no_word",
      "unavailable",
      "invalid_words",
      "dictionary",
      "score_overflow",
    ]),
    words: z.array(z.string().min(1).max(60)).max(31),
  }),
]);
export const turnAction = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("commit"),
    words: z
      .array(
        // A spelled word covers at most 15 cells of at most 4-byte faces; the configured
        // ruleset bounds it further (see `publicSnapshot`).
        z
          .string()
          .min(1)
          .max(60)
          .refine((word) => Array.from(word).length <= 15 * MAX_FACE_BYTES),
      )
      .max(31),
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
  analysis: moveAnalysis.optional(),
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
export const ruleset = z
  .strictObject({
    identity,
    dictionary: identity,
    tiles: z
      .array(
        z.strictObject({
          face: tileFace,
          count: z.int().min(1).max(65535),
          value: z.int().min(0).max(65535),
        }),
      )
      .min(1)
      .max(64),
    board_size: z.int().min(1).max(15),
    board_shape: z.enum(["square", "hexagon"]).optional(),
    premium_policy: z.enum(["all_word", "main_word_only"]).optional(),
    main_word_length: z.enum(["tiles", "letters"]).optional(),
    premiums: z.array(premium).max(225),
    rack_size: z.int().min(1).max(15),
    bingo_bonus: z.int().min(0).max(65535),
    exchange_minimum_bag: z.int().min(0).max(65535),
    scoreless_turn_limit: z.int().min(1).max(65535),
    minimum_players: z.int().min(2).max(4),
    maximum_players: z.int().min(2).max(4),
  })
  .refine(
    (value) =>
      value.board_size % 2 === 1 &&
      value.premiums.length ===
        (value.board_shape === "hexagon"
          ? 3 * Math.floor(value.board_size / 2) * (Math.floor(value.board_size / 2) + 1) + 1
          : value.board_size ** 2) &&
      (value.board_shape === "hexagon"
        ? value.board_size >= 3
        : (value.premium_policy ?? "all_word") === "all_word" &&
          (value.main_word_length ?? "tiles") === "tiles") &&
      value.minimum_players <= value.maximum_players,
  )
  .refine((value) => {
    const faces = value.tiles.map((tile) => faceText(tile.face) ?? "");
    const total = value.tiles.reduce((sum, tile) => sum + tile.count, 0);
    return (
      new Set(faces).size === faces.length &&
      // Rust `validate_tiles` still requires at least one single-letter face (#116).
      value.tiles.some((tile) => tile.face.kind === "letter") &&
      value.tiles.every((tile) =>
        tile.face.kind === "blank"
          ? tile.value === 0
          : tile.face.kind === "letters" || /^\p{Alphabetic}$/u.test(tile.face.letter),
      ) &&
      total >= value.maximum_players * value.rack_size &&
      total <= 200
    );
  });
/** Longest spelled word the ruleset allows: every cell of a line holds the longest face. */
export function maxWordLength(rules: Ruleset): number {
  const longest = Math.max(
    1,
    ...rules.tiles.map((tile) => Array.from(faceText(tile.face) ?? "").length),
  );
  return rules.board_size * longest;
}
const snapshotFields = z.strictObject({
  version: z.literal(1),
  game_id: id,
  revision: id,
  ruleset: identity,
  configuration: ruleset,
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
  preview: z
    .strictObject({
      player_id: id,
      turn: id,
      tiles: z.array(tile).max(15),
      evaluation: placementEvaluation.optional(),
    })
    .nullable(),
});
export const publicSnapshot = snapshotFields.refine((snapshot) => {
  const bound = maxWordLength(snapshot.configuration);
  return snapshot.history.every(
    (turn) =>
      turn.action.kind !== "commit" ||
      (turn.action.words.length <= (snapshot.configuration.board_shape === "hexagon" ? 31 : 16) &&
        turn.action.words.every((word) => Array.from(word).length <= bound)),
  );
});
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
