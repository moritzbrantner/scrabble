// Guest-owned custom word lists (#97; Decisions 1.1, 1.8, 1.16, 1.18, 1.20, 1.25, 1.45).
// The server parses, validates and stores word lists; this module only transports text
// and keeps the private owner capability in this browser's localStorage, bound to the
// game API that issued it (the same model as the board-layout library, #101).
import { z } from "zod";

import { gameApiUrl, readResponse } from "./create-game";
import { canonicalApi, shareUrl } from "./layout-library";

const identity = z.strictObject({ name: z.string(), revision: z.string() });
export type WordListIdentity = z.infer<typeof identity>;
const shareId = z.string().regex(/^s_[0-9a-f]{32}$/);
const savedWordList = z.object({
  version: z.literal(1),
  wordListId: z.string().regex(/^w_[0-9a-f]{32}$/),
  name: z.string(),
  revision: z.int(),
  wordList: identity,
  wordCount: z.int().min(0),
  shareId: shareId.optional(),
});
export type SavedWordList = z.infer<typeof savedWordList>;
const ownedWordList = savedWordList.extend({ words: z.string() });
export type OwnedWordList = z.infer<typeof ownedWordList>;
const createdWordList = savedWordList.extend({
  ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
});
const sharedWordList = z.object({
  version: z.literal(1),
  shareId,
  name: z.string(),
  revision: z.int(),
  wordList: identity,
  wordCount: z.int().min(0),
});
export type SharedWordList = z.infer<typeof sharedWordList>;
const limitsSchema = z.object({
  version: z.literal(1),
  maxBytes: z.int().positive(),
  maxWords: z.int().positive(),
});
export type WordListLimits = z.infer<typeof limitsSchema>;

export class WordListError extends Error {
  constructor(
    readonly code: "invalid" | "tooLarge" | "owner" | "unknown" | "full" | "storage" | "failed",
    readonly reason?: string,
    readonly line?: number,
  ) {
    super(code);
  }
}

const ownerPrefix = "scrabble:word-list-owner:v1:";
const ownerEntry = z.strictObject({
  version: z.literal(1),
  wordListId: z.string().regex(/^w_[0-9a-f]{32}$/),
  ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
  /** Canonical game API that issued the capability; it is never sent anywhere else. */
  api: z.string(),
});
export type WordListOwner = z.infer<typeof ownerEntry>;

/** Owned originals issued by `api` and known to this browser. */
export function ownedWordLists(api: string): WordListOwner[] {
  const entries: WordListOwner[] = [];
  let current: string;
  try {
    current = canonicalApi(api);
  } catch {
    return entries;
  }
  try {
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (key?.startsWith(ownerPrefix)) {
        const parsed = ownerEntry.safeParse(JSON.parse(localStorage.getItem(key) ?? "null"));
        if (
          parsed.success &&
          key === ownerPrefix + parsed.data.wordListId &&
          parsed.data.api === current
        ) {
          entries.push(parsed.data);
        }
      }
    }
  } catch {
    return entries;
  }
  return entries.sort((a, b) => a.wordListId.localeCompare(b.wordListId));
}
function issuedBy(api: string, entry: WordListOwner) {
  if (entry.api !== canonicalApi(api)) {
    throw new WordListError("owner");
  }
}
/** Fails before any server-side creation when this browser cannot keep a capability. */
function storageAvailable() {
  try {
    const probe = `${ownerPrefix}probe`;
    localStorage.setItem(probe, "1");
    localStorage.removeItem(probe);
  } catch {
    throw new WordListError("storage");
  }
}
/** Stores a newly issued capability; if that fails, deletes the just-created original
 * with the in-memory capability so no unmanageable list is left behind. */
async function keep(api: string, created: z.infer<typeof createdWordList>) {
  const entry: WordListOwner = {
    version: 1,
    wordListId: created.wordListId,
    ownerCapability: created.ownerCapability,
    api: canonicalApi(api),
  };
  try {
    localStorage.setItem(ownerPrefix + entry.wordListId, JSON.stringify(entry));
  } catch {
    await request(api, `/word-lists/${entry.wordListId}/delete`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
    }).catch(() => undefined);
    throw new WordListError("storage");
  }
}
/** Removes this browser's entry after its issuing API deleted or no longer knows it. */
export function forgetWordList(entry: WordListOwner) {
  try {
    localStorage.removeItem(ownerPrefix + entry.wordListId);
  } catch {
    // Storage may be unavailable; nothing else holds the capability.
  }
}

function libraryUrl(api: string, path: string): URL {
  const url = gameApiUrl(api);
  url.pathname = url.pathname.replace(/\/games$/, path);
  return url;
}
/** Responses carry at most one list's words (bounded by the published limit). */
const MAX_RESPONSE_BYTES = 8 * 1024 * 1024;
async function request(api: string, path: string, body?: unknown): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(libraryUrl(api, path), {
      method: body === undefined ? "GET" : "POST",
      credentials: "omit",
      ...(body === undefined
        ? {}
        : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }),
      signal: AbortSignal.timeout(15_000),
    });
  } catch {
    throw new WordListError("failed");
  }
  if (response.status === 204) {
    await response.body?.cancel();
    return null;
  }
  const payload = await readResponse(response, MAX_RESPONSE_BYTES).catch(() => null);
  if (response.ok) {
    return payload;
  }
  const failure = z
    .object({ error: z.string(), reason: z.string().optional(), line: z.int().optional() })
    .safeParse(payload);
  const code = failure.success ? failure.data.error : "";
  const reason = failure.success ? failure.data.reason : undefined;
  if (code === "invalid-word-list") {
    throw new WordListError("invalid", reason, failure.success ? failure.data.line : undefined);
  }
  if (code === "word-list-too-large" || response.status === 413) {
    throw new WordListError("tooLarge", reason);
  }
  if (code === "word-list-owner-rejected") {
    throw new WordListError("owner");
  }
  if (code === "unknown-word-list" || code === "word-list-share-withdrawn") {
    throw new WordListError("unknown");
  }
  if (code === "word-list-library-full") {
    throw new WordListError("full");
  }
  throw new WordListError("failed");
}

export async function fetchWordListLimits(api: string): Promise<WordListLimits> {
  return limitsSchema.parse(await request(api, "/word-lists/limits"));
}
export async function createWordList(api: string, name: string, words: string) {
  storageAvailable();
  const created = createdWordList.parse(
    await request(api, "/word-lists", { version: 1, name, words }),
  );
  await keep(api, created);
  return savedWordList.parse(created);
}
export async function readWordList(api: string, entry: WordListOwner): Promise<OwnedWordList> {
  issuedBy(api, entry);
  return ownedWordList.parse(
    await request(api, `/word-lists/${entry.wordListId}/read`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
    }),
  );
}
export async function updateWordList(
  api: string,
  entry: WordListOwner,
  name: string,
  words: string,
): Promise<OwnedWordList> {
  issuedBy(api, entry);
  return ownedWordList.parse(
    await request(api, `/word-lists/${entry.wordListId}/update`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
      name,
      words,
    }),
  );
}
export async function deleteWordList(api: string, entry: WordListOwner) {
  issuedBy(api, entry);
  await request(api, `/word-lists/${entry.wordListId}/delete`, {
    version: 1,
    ownerCapability: entry.ownerCapability,
  });
  forgetWordList(entry);
}
export async function shareWordList(api: string, entry: WordListOwner): Promise<string> {
  issuedBy(api, entry);
  return z.object({ version: z.literal(1), shareId }).parse(
    await request(api, `/word-lists/${entry.wordListId}/share`, {
      version: 1,
      ownerCapability: entry.ownerCapability,
    }),
  ).shareId;
}
export async function unshareWordList(api: string, entry: WordListOwner) {
  issuedBy(api, entry);
  await request(api, `/word-lists/${entry.wordListId}/unshare`, {
    version: 1,
    ownerCapability: entry.ownerCapability,
  });
}
export async function fetchSharedWordList(api: string, id: string): Promise<SharedWordList> {
  if (!shareId.safeParse(id).success) {
    throw new WordListError("unknown");
  }
  return sharedWordList.parse(await request(api, `/shared-word-lists/${id}`));
}
export async function copySharedWordList(api: string, id: string, name: string) {
  storageAvailable();
  const created = createdWordList.parse(
    await request(api, `/shared-word-lists/${id}/copy`, { version: 1, name }),
  );
  await keep(api, created);
  return savedWordList.parse(created);
}

/** Share URL: only `view=word-list`, the share id and the dev-only connection parameters. */
export function wordListShareUrl(id: string): string {
  return shareUrl(id, "word-list");
}

/** A selectable word list: an operator catalog row, a saved original or an opened share. */
export type WordChoice = { value: string; label: string; identity: WordListIdentity };

export type ImportResult =
  | { kind: "text"; text: string }
  | { kind: "tooLarge" }
  | { kind: "notText" }
  | { kind: "empty" };
/** Decodes a chosen file strictly as UTF-8 before anything is sent; the server remains
 * the only authority on the word-list format. */
export async function importWordListFile(
  file: Blob,
  limits: WordListLimits | undefined,
): Promise<ImportResult> {
  if (limits !== undefined && file.size > limits.maxBytes) {
    return { kind: "tooLarge" };
  }
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(
      await file.arrayBuffer(),
    );
  } catch {
    return { kind: "notText" };
  }
  if (text.trim() === "") {
    return { kind: "empty" };
  }
  return { kind: "text", text };
}
