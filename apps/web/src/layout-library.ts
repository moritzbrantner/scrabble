// Guest-owned custom board layouts (#101; Decisions 1.33, 1.35, 1.37, 1.45).
// The server validates and stores layouts; this module only transports documents
// and keeps the private owner capability in this browser's localStorage.
import { z } from "zod";

import { gameApiUrl, readResponse } from "./create-game";
import { type ProfileCatalog } from "./profile-model";
import { premium } from "./public-state";

export type Premium = z.infer<typeof premium>;
export type BoardRow = ProfileCatalog["boardProfiles"][number];
export const layoutDocument = z.strictObject({
  boardShape: z.enum(["square", "hexagon"]),
  boardSize: z.int().min(1).max(15),
  premiums: z.array(premium).max(225),
  premiumPolicy: z.enum(["all_word", "main_word_only"]).optional(),
  mainWordLength: z.enum(["tiles", "letters"]).optional(),
});
export type LayoutDocument = z.infer<typeof layoutDocument>;
const identity = z.strictObject({ name: z.string(), revision: z.string() });
const savedLayout = z.object({
  version: z.literal(1),
  layoutId: z.string().regex(/^l_[0-9a-f]{32}$/),
  name: z.string(),
  revision: z.int(),
  board: identity,
  layout: layoutDocument,
  shareId: z
    .string()
    .regex(/^s_[0-9a-f]{32}$/)
    .optional(),
});
export type SavedLayout = z.infer<typeof savedLayout>;
const createdLayout = savedLayout.extend({
  ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
});
const sharedLayout = z.object({
  version: z.literal(1),
  shareId: z.string().regex(/^s_[0-9a-f]{32}$/),
  name: z.string(),
  revision: z.int(),
  board: identity,
  layout: layoutDocument,
});
export type SharedLayout = z.infer<typeof sharedLayout>;

export class LayoutError extends Error {
  constructor(
    readonly code: "invalid" | "owner" | "unknown" | "full" | "failed",
    readonly reason?: string,
  ) {
    super(code);
  }
}

const ownerPrefix = "scrabble:layout-owner:v1:";
const ownerEntry = z.strictObject({
  version: z.literal(1),
  layoutId: z.string().regex(/^l_[0-9a-f]{32}$/),
  ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
});
export type OwnerEntry = z.infer<typeof ownerEntry>;

/** Owned originals known to this browser. The capability never leaves localStorage except
 * in request bodies to the layout API. */
export function ownedEntries(): OwnerEntry[] {
  const entries: OwnerEntry[] = [];
  try {
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (key?.startsWith(ownerPrefix)) {
        const parsed = ownerEntry.safeParse(JSON.parse(localStorage.getItem(key) ?? "null"));
        if (parsed.success && key === ownerPrefix + parsed.data.layoutId) {
          entries.push(parsed.data);
        }
      }
    }
  } catch {
    return entries;
  }
  return entries.sort((a, b) => a.layoutId.localeCompare(b.layoutId));
}
function remember(entry: OwnerEntry) {
  localStorage.setItem(ownerPrefix + entry.layoutId, JSON.stringify(entry));
}
export function forget(layoutId: string) {
  try {
    localStorage.removeItem(ownerPrefix + layoutId);
  } catch {
    // Storage may be unavailable; nothing else holds the capability.
  }
}

function layoutUrl(api: string, path: string): URL {
  const url = gameApiUrl(api);
  url.pathname = url.pathname.replace(/\/games$/, path);
  return url;
}
async function request(api: string, path: string, body?: unknown): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(layoutUrl(api, path), {
      method: body === undefined ? "GET" : "POST",
      credentials: "omit",
      ...(body === undefined
        ? {}
        : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }),
      signal: AbortSignal.timeout(8000),
    });
  } catch {
    throw new LayoutError("failed");
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
  if (code === "invalid-layout") {
    throw new LayoutError("invalid", failure.success ? failure.data.reason : undefined);
  }
  if (code === "layout-owner-rejected") {
    throw new LayoutError("owner");
  }
  if (code === "unknown-layout") {
    throw new LayoutError("unknown");
  }
  if (code === "layout-library-full") {
    throw new LayoutError("full");
  }
  throw new LayoutError("failed");
}

export async function createLayout(api: string, name: string, layout: LayoutDocument) {
  const created = createdLayout.parse(await request(api, "/layouts", { version: 1, name, layout }));
  remember({ version: 1, layoutId: created.layoutId, ownerCapability: created.ownerCapability });
  return savedLayout.parse(created);
}
export async function readLayout(api: string, entry: OwnerEntry): Promise<SavedLayout> {
  return savedLayout.parse(
    await request(api, `/layouts/${entry.layoutId}/read`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
    }),
  );
}
export async function updateLayout(
  api: string,
  entry: OwnerEntry,
  name: string,
  layout: LayoutDocument,
): Promise<SavedLayout> {
  return savedLayout.parse(
    await request(api, `/layouts/${entry.layoutId}/update`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
      name,
      layout,
    }),
  );
}
export async function deleteLayout(api: string, entry: OwnerEntry) {
  await request(api, `/layouts/${entry.layoutId}/delete`, {
    version: 1,
    ownerCapability: entry.ownerCapability,
  });
  forget(entry.layoutId);
}
export async function shareLayout(api: string, entry: OwnerEntry): Promise<string> {
  const shared = z
    .object({ version: z.literal(1), shareId: z.string().regex(/^s_[0-9a-f]{32}$/) })
    .parse(
      await request(api, `/layouts/${entry.layoutId}/share`, {
        version: 1,
        ownerCapability: entry.ownerCapability,
      }),
    );
  return shared.shareId;
}
export async function fetchShared(api: string, shareId: string): Promise<SharedLayout> {
  if (!/^s_[0-9a-f]{32}$/.test(shareId)) {
    throw new LayoutError("unknown");
  }
  return sharedLayout.parse(await request(api, `/shared-layouts/${shareId}`));
}
export async function copyShared(api: string, shareId: string, name: string) {
  const created = createdLayout.parse(
    await request(api, `/shared-layouts/${shareId}/copy`, { version: 1, name }),
  );
  remember({ version: 1, layoutId: created.layoutId, ownerCapability: created.ownerCapability });
  return savedLayout.parse(created);
}

/** A saved version as a "Board layout" option row, labelled with the layout name. */
export function boardRow(name: string, board: SavedLayout["board"], layout: LayoutDocument) {
  const row: BoardRow = {
    identity: board,
    label: name,
    boardSize: layout.boardSize,
    premiums: layout.premiums,
  };
  if (layout.boardShape === "hexagon") {
    row.boardShape = "hexagon";
    row.premiumPolicy = layout.premiumPolicy ?? "all_word";
    row.mainWordLength = layout.mainWordLength ?? "tiles";
  }
  return row;
}

/** Share URL: only the view, the share id and the dev-only connection parameters. */
export function shareUrl(shareId: string): string {
  const current = new URL(window.location.href);
  const next = new URL(current.pathname, current.origin);
  for (const key of ["server", "api", "certificate"]) {
    const value = current.searchParams.get(key);
    if (value !== null) {
      next.searchParams.set(key, value);
    }
  }
  next.searchParams.set("view", "layout");
  next.searchParams.set("share", shareId);
  return next.href;
}

// Pure geometry helpers for the editor (presentation only; the server validates).
export function contains(shape: "square" | "hexagon", size: number, row: number, column: number) {
  const radius = Math.floor(size / 2);
  return (
    row >= 0 &&
    column >= 0 &&
    row < size &&
    column < size &&
    (shape !== "hexagon" || (row + column >= radius && row + column <= 3 * radius))
  );
}
export function cells(shape: "square" | "hexagon", size: number) {
  const result: { row: number; column: number; index: number }[] = [];
  for (let row = 0; row < size; row += 1) {
    for (let column = 0; column < size; column += 1) {
      if (contains(shape, size, row, column)) {
        result.push({ row, column, index: result.length });
      }
    }
  }
  return result;
}
export function validSize(size: number) {
  return Number.isInteger(size) && size % 2 === 1 && size >= 3 && size <= 15;
}
/** Re-centres premiums onto another complete regular grid; new cells are normal. */
export function reshape(
  from: { shape: "square" | "hexagon"; size: number; premiums: readonly Premium[] },
  shape: "square" | "hexagon",
  size: number,
): Premium[] {
  const offset = (size - from.size) / 2;
  const old = new Map(
    cells(from.shape, from.size).map((cell) => [`${cell.row},${cell.column}`, cell.index]),
  );
  return cells(shape, size).map((cell) => {
    const index = old.get(`${cell.row - offset},${cell.column - offset}`);
    return index === undefined ? "normal" : (from.premiums[index] ?? "normal");
  });
}
