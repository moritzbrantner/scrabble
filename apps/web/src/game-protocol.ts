import { z } from "zod";
import { coordinate, id, letter, playerSnapshot, type PlayerSnapshot } from "./public-state";
import { ProtocolError } from "./transport/wire";

const placements = z
  .array(
    z.strictObject({
      tile_id: id,
      coordinate,
      blank_as: letter.nullable(),
    }),
  )
  .max(15);
export const commandEnvelope = z.strictObject({
  version: z.literal(1),
  game_id: id,
  player_id: id,
  sequence: z.int().min(1).max(0xffffffff),
  expected_turn: id,
  command: z.discriminatedUnion("kind", [
    z.strictObject({ kind: z.literal("start") }),
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
