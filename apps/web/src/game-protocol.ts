import { z } from "zod";
import { coordinate, id, letter, playerSnapshot, type PlayerSnapshot } from "./public-state";
import { ProtocolError } from "./transport/wire";

export const placement = z.strictObject({ tile_id: id, coordinate, blank_as: letter.nullable() });
const placements = z.array(placement).max(15);
export const commandEnvelope = z.strictObject({
  version: z.literal(1),
  game_id: id,
  player_id: id,
  sequence: z.int().min(1).max(0xffffffff),
  expected_turn: id,
  command: z.discriminatedUnion("kind", [
    z.strictObject({ kind: z.literal("start") }),
    z.strictObject({
      kind: z.literal("claim_board"),
      request_id: z.string().regex(/^[0-9a-f]{32}$/),
    }),
    z.strictObject({ kind: z.literal("set_name"), display_name: z.string().max(128) }),
    z.strictObject({ kind: z.literal("preview"), placements }),
    z.strictObject({ kind: z.literal("commit"), placements }),
    z.strictObject({ kind: z.literal("pass") }),
    z.strictObject({ kind: z.literal("exchange"), tile_ids: z.array(id).max(15) }),
  ]),
});
export type CommandEnvelope = z.infer<typeof commandEnvelope>;

export function encodeGameCommand(command: CommandEnvelope): Uint8Array {
  const parsed = commandEnvelope.safeParse(command);
  if (!parsed.success) {
    throw new ProtocolError("Invalid or incompatible Scrabble command protocol");
  }
  return new TextEncoder().encode(JSON.stringify(parsed.data));
}

export function decodePlayerSnapshot(bytes: Uint8Array, playerId: string): PlayerSnapshot {
  let raw: unknown;
  try {
    raw = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    throw new ProtocolError("Invalid Scrabble snapshot encoding");
  }
  const parsed = playerSnapshot.safeParse(raw);
  if (!parsed.success) {
    throw new ProtocolError("Invalid or incompatible Scrabble snapshot protocol");
  }
  if (parsed.data.own_rack.player_id !== playerId) {
    throw new ProtocolError("Snapshot belongs to a different player");
  }
  return parsed.data;
}

const wordRejection = z.strictObject({
  version: z.literal(1),
  game_id: id,
  expected_turn: id,
  error: z.strictObject({
    kind: z.literal("invalid_words"),
    words: z
      .array(
        z
          .string()
          .min(1)
          .max(30)
          .refine((word) => Array.from(word).length <= 15 && /^\p{L}+$/u.test(word)),
      )
      .min(1)
      .max(16)
      .refine((words) => new Set(words).size === words.length),
  }),
});
export type WordRejection = z.infer<typeof wordRejection>;
export function decodeWordRejection(bytes: Uint8Array): WordRejection {
  let raw: unknown;
  try {
    raw = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    throw new ProtocolError("Invalid word rejection encoding");
  }
  const parsed = wordRejection.safeParse(raw);
  if (!parsed.success) {
    throw new ProtocolError("Invalid word rejection protocol");
  }
  return parsed.data;
}
