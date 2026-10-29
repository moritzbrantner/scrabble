// Optional linking of guest-owned libraries to the shared Moenarch account (#132;
// Decision 1.46). The game API verifies the account service's `moenarch_session` cookie;
// this module never sees the session token or names an account itself. Owner capabilities
// stay in localStorage exactly as for guests; reconnecting stores a new device capability.
import { z } from "zod";

import { gameApiUrl, readResponse } from "./create-game";
import { ownedEntries, rememberLayout } from "./layout-library";
import { ownedTileProfiles, rememberTileProfile } from "./tile-profile-library";
import { ownedWordLists, rememberWordList } from "./word-list-library";

export type LibraryKind = "wordList" | "tileProfile" | "layout";

export class AccountError extends Error {
  constructor(
    readonly code: "signedOut" | "unavailable" | "conflict" | "notOwner" | "failed",
    /** The account page, when signed out. */
    readonly signIn?: string,
  ) {
    super(code);
  }
}

const session = z.object({
  version: z.literal(1),
  accountId: z.string(),
  displayName: z.string(),
});
export type AccountSession = z.infer<typeof session>;
const linked = z.array(z.object({ id: z.string(), name: z.string() }));
const libraries = z.object({
  version: z.literal(1),
  wordLists: linked,
  tileProfiles: linked,
  layouts: linked,
});
const linkResult = z.object({ linked: z.enum(["new", "existing"]) });
const reconnectResult = z.object({ ownerCapability: z.string().regex(/^[0-9a-f]{64}$/) });

async function request(api: string, path: string, body: unknown): Promise<unknown> {
  const url = gameApiUrl(api);
  url.pathname = url.pathname.replace(/\/games$/, path);
  let response: Response;
  try {
    response = await fetch(url, {
      method: "POST",
      // The account cookie belongs to the account service's domain; send it to the API.
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ version: 1, ...(body as object) }),
      signal: AbortSignal.timeout(8000),
    });
  } catch {
    throw new AccountError("failed");
  }
  const payload = await readResponse(response, 64 * 1024).catch(() => null);
  if (response.ok) {
    return payload;
  }
  const failure = z.object({ error: z.string(), signIn: z.string().optional() }).safeParse(payload);
  const code = failure.success ? failure.data.error : "";
  const known: Record<string, AccountError["code"]> = {
    "signed-out": "signedOut",
    "accounts-unavailable": "unavailable",
    // A guest-only board origin (the GitHub Pages demo, #206) offers no account linking.
    "guest-origin": "unavailable",
    "account-link-conflict": "conflict",
    "account-not-owner": "notOwner",
  };
  const signIn = failure.success ? safeSignIn(failure.data.signIn) : undefined;
  throw new AccountError(known[code] ?? "failed", signIn);
}
/** Only an absolute https (or local http) page is offered as the sign-in link. */
function safeSignIn(value: string | undefined): string | undefined {
  try {
    const url = new URL(value ?? "");
    return url.protocol === "https:" ||
      (url.protocol === "http:" && ["localhost", "127.0.0.1"].includes(url.hostname))
      ? url.href
      : undefined;
  } catch {
    return undefined;
  }
}

export async function accountSession(api: string): Promise<AccountSession> {
  return session.parse(await request(api, "/account/session", {}));
}

/** A library original whose guest capability this browser holds. */
export type LocalOwner = { kind: LibraryKind; id: string; ownerCapability: string };
export function localOwners(api: string): LocalOwner[] {
  return [
    ...ownedWordLists(api).map((entry) => ({
      kind: "wordList" as const,
      id: entry.wordListId,
      ownerCapability: entry.ownerCapability,
    })),
    ...ownedTileProfiles(api).map((entry) => ({
      kind: "tileProfile" as const,
      id: entry.profileId,
      ownerCapability: entry.ownerCapability,
    })),
    ...ownedEntries(api).map((entry) => ({
      kind: "layout" as const,
      id: entry.layoutId,
      ownerCapability: entry.ownerCapability,
    })),
  ];
}

export type LinkSummary = { linked: number; existing: number; conflicts: number };
/** Links every library this browser owns to the signed-in account. */
export async function linkLocalLibraries(api: string): Promise<LinkSummary> {
  const summary: LinkSummary = { linked: 0, existing: 0, conflicts: 0 };
  for (const owner of localOwners(api)) {
    try {
      const result = linkResult.parse(await request(api, "/account/link", owner));
      summary[result.linked === "new" ? "linked" : "existing"] += 1;
    } catch (error) {
      if (error instanceof AccountError && error.code === "conflict") {
        summary.conflicts += 1;
      } else {
        throw error;
      }
    }
  }
  return summary;
}

/** Obtains device capabilities for linked libraries this browser does not hold yet.
 * Returns how many became available here. */
export async function reconnectLinkedLibraries(api: string): Promise<number> {
  const listed = libraries.parse(await request(api, "/account/libraries", {}));
  const held = new Set(localOwners(api).map((owner) => `${owner.kind}:${owner.id}`));
  const remember: Record<LibraryKind, (api: string, id: string, capability: string) => void> = {
    wordList: rememberWordList,
    tileProfile: rememberTileProfile,
    layout: rememberLayout,
  };
  let reconnected = 0;
  for (const [kind, rows] of [
    ["wordList", listed.wordLists],
    ["tileProfile", listed.tileProfiles],
    ["layout", listed.layouts],
  ] as const) {
    for (const { id } of rows) {
      if (held.has(`${kind}:${id}`)) {
        continue;
      }
      const { ownerCapability } = reconnectResult.parse(
        await request(api, "/account/reconnect", { kind, id }),
      );
      remember[kind](api, id, ownerCapability);
      reconnected += 1;
    }
  }
  return reconnected;
}
