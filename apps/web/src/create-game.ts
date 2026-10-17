import { z } from "zod";

import { copy, translate, type Copy, type CopyKey } from "./copy";
import { matchSettings, profileSelection } from "./profile-model";
import { id } from "./public-state";
import { timeControl } from "./time-control";
import { matchUrl } from "./transport/browser-match";

const creationIntent = z.strictObject({
  version: z.literal(2),
  requestId: z.string().regex(/^[0-9a-f]{32}$/),
  requestedAt: z.int().min(0).max(Number.MAX_SAFE_INTEGER),
});
/** Proof of access to a guest word list for a new match (#97, Decision 1.48). */
export const wordListAccess = z.union([
  z.strictObject({ shareId: z.string().regex(/^s_[0-9a-f]{32}$/) }),
  z.strictObject({
    wordListId: z.string().regex(/^w_[0-9a-f]{32}$/),
    ownerCapability: z.string().regex(/^[0-9a-f]{64}$/),
  }),
]);
export type WordListAccess = z.infer<typeof wordListAccess>;
const createRequest = z.discriminatedUnion("version", [
  creationIntent,
  creationIntent.extend({
    version: z.literal(3),
    selection: profileSelection.extend({ wordListAccess: wordListAccess.optional() }),
    rules: matchSettings,
    timeControl: timeControl.optional(),
  }),
]);
export type CreateRequest = z.infer<typeof createRequest>;
const joinInformation = z.strictObject({
  version: z.literal(1),
  matchId: z.string().regex(/^[gbp]_[0-9a-f]{16}_[0-9a-f]{32}$/),
  gameId: id,
  matchPath: z.string(),
  expiresAt: z.int().min(0).max(Number.MAX_SAFE_INTEGER),
});
export type JoinInformation = z.infer<typeof joinInformation>;

export class CreationError extends Error {
  readonly copy: Copy;
  constructor(
    key: CopyKey,
    readonly retryable: boolean,
  ) {
    super(translate("en", key));
    this.copy = copy(key);
  }
}
export function newCreateRequest(): CreateRequest {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return {
    version: 2,
    requestId: Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(""),
    requestedAt: Math.floor(Date.now() / 1000),
  };
}
export function decodeJoinInformation(raw: unknown, endpoint: string): JoinInformation {
  const parsed = joinInformation.safeParse(raw);
  if (
    !parsed.success ||
    matchUrl(endpoint, parsed.data.matchId).pathname !== parsed.data.matchPath
  ) {
    throw new CreationError("create.invalidInformation", false);
  }
  return parsed.data;
}
export function gameApiUrl(endpoint: string): URL {
  if (!/^https?:\/\/[^/?#\\]+(?:\/[A-Za-z0-9_-]+)*$/.test(endpoint)) {
    throw new CreationError("create.invalidSettings", false);
  }
  const url = new URL(endpoint);
  if (
    url.username ||
    url.password ||
    (url.protocol === "http:" && !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname))
  ) {
    throw new CreationError("create.invalidSettings", false);
  }
  url.pathname = `${url.pathname === "/" ? "" : url.pathname}/games`;
  return url;
}
export async function readResponse(response: Response, limit = 4096): Promise<unknown> {
  if (response.body === null) {
    throw new CreationError("create.missingInformation", true);
  }
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      length += value.length;
      if (length > limit) {
        throw new CreationError("create.invalidInformation", false);
      }
      chunks.push(value);
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    try {
      return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
    } catch {
      throw new CreationError("create.invalidInformation", false);
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
/** The caller retains one request across failures; no retry key or reconnect capability enters storage/URLs. */
export async function createGame(
  api: string,
  endpoint: string,
  request: CreateRequest,
  signal: AbortSignal,
): Promise<JoinInformation> {
  const url = gameApiUrl(api);
  try {
    matchUrl(endpoint, "validation");
  } catch {
    throw new CreationError("create.invalidConnection", false);
  }
  const body = createRequest.parse(request);
  try {
    const response = await fetch(url, {
      method: "POST",
      credentials: "omit",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.any([signal, AbortSignal.timeout(5000)]),
    });
    if (!response.ok) {
      if (body.version === 3 && (response.status === 400 || response.status === 409)) {
        const failure = z.object({ error: z.string() }).safeParse(await readResponse(response));
        const code = failure.success ? failure.data.error : "invalid-request";
        if (code === "invalid-time-control") {
          throw new CreationError("clock.invalid", false);
        }
        if (code === "unknown-profile") {
          throw new CreationError("profiles.unknown", false);
        }
        if (code === "incompatible-profiles") {
          throw new CreationError("profiles.incompatible", false);
        }
        if (code === "creation-conflict") {
          throw new CreationError("profiles.conflict", false);
        }
        throw new CreationError("profiles.invalidRules", false);
      }
      await response.body?.cancel();
      switch (response.status) {
        case 410:
          throw new CreationError("create.expired", false);
        case 429:
          throw new CreationError("create.full", true);
        case 503:
          throw new CreationError("create.unavailable", true);
        case 400:
          throw new CreationError("create.rejected", false);
        case 403:
          throw new CreationError("create.forbidden", false);
        default:
          throw new CreationError("create.retryable", true);
      }
    }
    return decodeJoinInformation(await readResponse(response), endpoint);
  } catch (error) {
    if (error instanceof CreationError) {
      throw error;
    }
    throw new CreationError("create.interrupted", true);
  }
}

export class JoinError extends Error {
  readonly copy: Copy;
  constructor(key: CopyKey) {
    super(translate("en", key));
    this.copy = copy(key);
  }
}

/** Public existence/lifetime check; never allocates a match or returns credentials. */
export async function lookupGame(
  api: string,
  endpoint: string,
  matchId: string,
  signal: AbortSignal,
  mode: "available" | "new" = "available",
): Promise<JoinInformation> {
  if (!joinInformation.shape.matchId.safeParse(matchId).success) {
    throw new JoinError("join.invalidId");
  }
  matchUrl(endpoint, matchId);
  const url = gameApiUrl(api);
  url.pathname += `/${matchId}${mode === "new" ? "/join" : ""}`;
  const response = await fetch(url, {
    credentials: "omit",
    signal: AbortSignal.any([signal, AbortSignal.timeout(5000)]),
  });
  if (!response.ok) {
    await response.body?.cancel();
    if (response.status === 429) {
      throw new JoinError("join.full");
    }
    if (response.status === 409) {
      throw new JoinError("join.started");
    }
    if (response.status === 410) {
      throw new JoinError("join.expired");
    }
    if (response.status === 404) {
      throw new JoinError("join.unavailable");
    }
    throw new JoinError("join.checkFailed");
  }
  const join = decodeJoinInformation(await readResponse(response), endpoint);
  if (join.matchId !== matchId) {
    throw new JoinError("join.differentGame");
  }
  return join;
}

/** A finished board's one-tap rematch (#33). The finished match's nonce proves board ownership. */
export async function startRematch(
  api: string,
  endpoint: string,
  finishedId: string,
  boardRequestId: string,
  request: CreateRequest,
  signal: AbortSignal,
): Promise<JoinInformation> {
  const url = gameApiUrl(api);
  url.pathname += `/${finishedId}/rematch`;
  const body = createRequest.parse(request);
  try {
    const response = await fetch(url, {
      method: "POST",
      credentials: "omit",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        version: 1,
        boardRequestId,
        requestId: body.requestId,
        requestedAt: body.requestedAt,
      }),
      signal: AbortSignal.any([signal, AbortSignal.timeout(5000)]),
    });
    if (!response.ok) {
      await response.body?.cancel();
      switch (response.status) {
        case 410:
          throw new CreationError("create.expired", false);
        case 404:
        case 403:
        case 409:
          throw new CreationError("rematch.unavailable", false);
        default:
          throw new CreationError("rematch.failed", true);
      }
    }
    return decodeJoinInformation(await readResponse(response), endpoint);
  } catch (error) {
    if (error instanceof CreationError) {
      throw error;
    }
    throw new CreationError("rematch.failed", true);
  }
}

/** Public discovery of a finished match's rematch; `undefined` while none exists. */
export async function lookupRematch(
  api: string,
  endpoint: string,
  finishedId: string,
  signal: AbortSignal,
): Promise<JoinInformation | undefined> {
  const url = gameApiUrl(api);
  url.pathname += `/${finishedId}/rematch`;
  const response = await fetch(url, {
    credentials: "omit",
    signal: AbortSignal.any([signal, AbortSignal.timeout(5000)]),
  });
  if (!response.ok) {
    await response.body?.cancel();
    return undefined;
  }
  return decodeJoinInformation(await readResponse(response), endpoint);
}
