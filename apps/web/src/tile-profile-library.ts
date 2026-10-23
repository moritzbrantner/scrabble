// Guest-owned private tile profiles (#100; Decisions 1.21, 1.25, 1.27, 1.29, 1.45).
// The server validates and stores profiles; this module only transports them and keeps
// the private owner capability in this browser's localStorage. There are no share links.
import { z } from "zod";

import { gameApiUrl, readResponse } from "./create-game";
import { canonicalApi } from "./layout-library";
import { ruleset } from "./public-state";

export type TileDefinition = z.infer<typeof ruleset.shape.tiles>[number];
export type TileRow = {
  identity: { name: string; revision: string };
  label: string;
  tiles: TileDefinition[];
};
const savedProfile = z.object({
  version: z.literal(1),
  profileId: z.string().regex(/^t_[0-9a-f]{32}$/),
  name: z.string(),
  revision: z.int(),
  identity: ruleset.shape.identity,
  tiles: ruleset.shape.tiles,
});
export type SavedTileProfile = z.infer<typeof savedProfile>;
const createdProfile = savedProfile.extend({
  ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
});

export class TileProfileError extends Error {
  constructor(
    readonly code: "invalid" | "owner" | "unknown" | "full" | "storage" | "failed",
    readonly reason?: string,
  ) {
    super(code);
  }
}

const ownerPrefix = "scrabble:tile-profile-owner:v1:";
const ownerEntry = z.strictObject({
  version: z.literal(1),
  profileId: z.string().regex(/^t_[0-9a-f]{32}$/),
  ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
  /** Canonical game API that issued the capability; it is never sent anywhere else. */
  api: z.string(),
});
export type TileProfileOwner = z.infer<typeof ownerEntry>;

/** Owned originals issued by `api` and known to this browser. */
export function ownedTileProfiles(api: string): TileProfileOwner[] {
  const entries: TileProfileOwner[] = [];
  try {
    const current = canonicalApi(api);
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (key?.startsWith(ownerPrefix)) {
        const parsed = ownerEntry.safeParse(JSON.parse(localStorage.getItem(key) ?? "null"));
        if (
          parsed.success &&
          key === ownerPrefix + parsed.data.profileId &&
          parsed.data.api === current
        ) {
          entries.push(parsed.data);
        }
      }
    }
  } catch {
    return entries;
  }
  return entries.sort((a, b) => a.profileId.localeCompare(b.profileId));
}
function issuedBy(api: string, entry: TileProfileOwner) {
  if (entry.api !== canonicalApi(api)) {
    throw new TileProfileError("owner");
  }
}
/** Removes this browser's entry after its issuing API deleted or no longer knows it. */
export function forgetTileProfile(entry: TileProfileOwner) {
  try {
    localStorage.removeItem(ownerPrefix + entry.profileId);
  } catch {
    // Storage may be unavailable; nothing else holds the capability.
  }
}

async function request(api: string, path: string, body: unknown): Promise<unknown> {
  const url = gameApiUrl(api);
  url.pathname = url.pathname.replace(/\/games$/, path);
  let response: Response;
  try {
    response = await fetch(url, {
      method: "POST",
      credentials: "omit",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(8000),
    });
  } catch {
    throw new TileProfileError("failed");
  }
  if (response.status === 204) {
    await response.body?.cancel();
    return null;
  }
  const payload = await readResponse(response, 64 * 1024).catch(() => null);
  if (response.ok) {
    return payload;
  }
  const failure = z.object({ error: z.string(), reason: z.string().optional() }).safeParse(payload);
  const code = failure.success ? failure.data.error : "";
  const known: Record<string, TileProfileError["code"]> = {
    "invalid-tile-profile": "invalid",
    "tile-profile-owner-rejected": "owner",
    "unknown-tile-profile": "unknown",
    "tile-profile-library-full": "full",
  };
  throw new TileProfileError(
    known[code] ?? "failed",
    failure.success ? failure.data.reason : undefined,
  );
}

export async function createTileProfile(api: string, name: string, tiles: TileDefinition[]) {
  try {
    const probe = `${ownerPrefix}probe`;
    localStorage.setItem(probe, "1");
    localStorage.removeItem(probe);
  } catch {
    throw new TileProfileError("storage");
  }
  const created = createdProfile.parse(
    await request(api, "/tile-profiles", { version: 1, name, tiles }),
  );
  const entry: TileProfileOwner = {
    version: 1,
    profileId: created.profileId,
    ownerCapability: created.ownerCapability,
    api: canonicalApi(api),
  };
  try {
    localStorage.setItem(ownerPrefix + entry.profileId, JSON.stringify(entry));
  } catch {
    // Without a stored capability the original would be unmanageable; remove it again.
    await request(api, `/tile-profiles/${entry.profileId}/delete`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
    }).catch(() => undefined);
    throw new TileProfileError("storage");
  }
  const { ownerCapability: _capability, ...saved } = created;
  return { entry, saved };
}
export async function readTileProfile(api: string, entry: TileProfileOwner) {
  issuedBy(api, entry);
  return savedProfile.parse(
    await request(api, `/tile-profiles/${entry.profileId}/read`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
    }),
  );
}
export async function updateTileProfile(
  api: string,
  entry: TileProfileOwner,
  name: string,
  tiles: TileDefinition[],
) {
  issuedBy(api, entry);
  return savedProfile.parse(
    await request(api, `/tile-profiles/${entry.profileId}/update`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
      name,
      tiles,
    }),
  );
}
export async function deleteTileProfile(api: string, entry: TileProfileOwner) {
  issuedBy(api, entry);
  await request(api, `/tile-profiles/${entry.profileId}/delete`, {
    version: 1,
    ownerCapability: entry.ownerCapability,
  });
  forgetTileProfile(entry);
}

/** One editable table row; faces are typed text, numbers are kept as typed. */
export type DraftRow = { face: string; count: string; value: string };
export type Draft = { name: string; rows: DraftRow[]; blanks: string };

export function draftFrom(name: string, tiles: TileDefinition[]): Draft {
  const rows: DraftRow[] = [];
  let blanks = "0";
  for (const tile of tiles) {
    if (tile.face.kind === "blank") {
      blanks = String(tile.count);
    } else {
      rows.push({
        face: tile.face.kind === "letter" ? tile.face.letter : tile.face.letters,
        count: String(tile.count),
        value: String(tile.value),
      });
    }
  }
  return { name, rows, blanks };
}

/** Converts the draft into tile definitions, or names the first row the client can reject
 * before asking the server (which stays the authority for every other rule). */
export function tilesFrom(
  draft: Draft,
): { tiles: TileDefinition[] } | { row: number } | { blanks: true } {
  const whole = (text: string) => (/^\d{1,5}$/.test(text.trim()) ? Number(text) : NaN);
  const tiles: TileDefinition[] = [];
  for (const [index, row] of draft.rows.entries()) {
    const face = row.face.trim().toLocaleUpperCase();
    const count = whole(row.count);
    const value = whole(row.value);
    if (
      !Number.isFinite(count) ||
      (count > 0 && (face === "" || /\s/.test(face) || !Number.isFinite(value)))
    ) {
      return { row: index + 1 };
    }
    if (count === 0) {
      continue;
    }
    tiles.push({
      face:
        Array.from(face).length === 1
          ? { kind: "letter", letter: face }
          : { kind: "letters", letters: face },
      count,
      value,
    });
  }
  const blanks = whole(draft.blanks);
  if (!Number.isFinite(blanks)) {
    return { blanks: true };
  }
  if (blanks > 0) {
    tiles.push({ face: { kind: "blank" }, count: blanks, value: 0 });
  }
  return { tiles };
}
