import { z } from "zod";

export const resumeCapability = z.strictObject({
  version: z.literal(1),
  route: z.string().max(1024),
  playerId: z
    .string()
    .regex(/^[1-9][0-9]{0,9}$/)
    .refine((value) => BigInt(value) <= 0xffffffffn),
  token: z.string().regex(/^[0-9a-f]{32}$/),
  sequence: z.int().min(0).max(0xffffffff),
});
export type ResumeCapability = z.infer<typeof resumeCapability>;

/** Call only at the private storage boundary, never for a URL or view projection. */
export function readPlayerSession(route: string): ResumeCapability | undefined {
  return readSession("player", route);
}

export function readBoardSession(route: string): ResumeCapability | undefined {
  return readSession("board", route);
}
function readSession(purpose: "player" | "board", route: string): ResumeCapability | undefined {
  const value = sessionStorage.getItem(`scrabble:${purpose}:v1:${route}`);
  if (value === null) {
    return undefined;
  }
  if (value.length > 2048) {
    throw new Error("Invalid saved player session.");
  }
  const parsed = resumeCapability.parse(JSON.parse(value));
  if (parsed.route !== route) {
    throw new Error("Saved player session belongs to another game.");
  }
  return parsed;
}
export function savePlayerSession(capability: ResumeCapability): void {
  sessionStorage.setItem(`scrabble:player:v1:${capability.route}`, JSON.stringify(capability));
}
export function forgetPlayerSession(route: string): void {
  sessionStorage.removeItem(`scrabble:player:v1:${route}`);
}

const storedBoardClaim = z.strictObject({
  version: z.literal(1),
  requestId: z.string().regex(/^[0-9a-f]{32}$/),
  gameId: z.string().max(64),
});
export type BoardClaim = { requestId: string; gameId: string };

/**
 * The private creation nonce, kept tab-only from the create response until the board's
 * ownership is confirmed, so a discarded tab can still claim its board (#218).
 */
export function saveBoardClaim(route: string, claim: BoardClaim): void {
  sessionStorage.setItem(
    `scrabble:board-claim:v1:${route}`,
    JSON.stringify({ version: 1, requestId: claim.requestId, gameId: claim.gameId }),
  );
}
export function readBoardClaim(route: string): BoardClaim | undefined {
  const value = sessionStorage.getItem(`scrabble:board-claim:v1:${route}`);
  if (value === null || value.length > 512) {
    return undefined;
  }
  try {
    const { requestId, gameId } = storedBoardClaim.parse(JSON.parse(value));
    return { requestId, gameId };
  } catch {
    return undefined;
  }
}
export function forgetBoardClaim(route: string): void {
  sessionStorage.removeItem(`scrabble:board-claim:v1:${route}`);
}

/** A board capsule is kept separate from phone identity even at the same match route. */
export function saveBoardSession(capability: ResumeCapability): void {
  sessionStorage.setItem(`scrabble:board:v1:${capability.route}`, JSON.stringify(capability));
}
