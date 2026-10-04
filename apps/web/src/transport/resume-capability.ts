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
  const value = sessionStorage.getItem(`scrabble:player:v1:${route}`);
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
