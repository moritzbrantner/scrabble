import { z } from "zod";
import { id } from "./public-state";
import { matchUrl } from "./transport/browser-match";

const createRequest = z.strictObject({
  version: z.literal(1),
  requestId: z.string().regex(/^[0-9a-f]{32}$/),
  requestedAt: z.int().min(0).max(Number.MAX_SAFE_INTEGER),
});
export type CreateRequest = z.infer<typeof createRequest>;
const joinInformation = z.strictObject({
  version: z.literal(1),
  matchId: z.string().regex(/^g_[0-9a-f]{16}_[0-9a-f]{32}$/),
  gameId: id,
  matchPath: z.string(),
  expiresAt: z.int().min(0).max(Number.MAX_SAFE_INTEGER),
});
export type JoinInformation = z.infer<typeof joinInformation>;

export class CreationError extends Error {
  constructor(
    message: string,
    readonly retryable: boolean,
  ) {
    super(message);
  }
}
export function newCreateRequest(): CreateRequest {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return {
    version: 1,
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
    throw new CreationError("The server returned invalid join information.", false);
  }
  return parsed.data;
}
function creationUrl(endpoint: string): URL {
  if (!/^https?:\/\/[^/?#\\]+(?:\/[A-Za-z0-9_-]+)*$/.test(endpoint)) {
    throw new CreationError("Invalid game creation settings.", false);
  }
  const url = new URL(endpoint);
  if (
    url.username ||
    url.password ||
    (url.protocol === "http:" && !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname))
  ) {
    throw new CreationError("Invalid game creation settings.", false);
  }
  url.pathname = `${url.pathname === "/" ? "" : url.pathname}/games`;
  return url;
}
async function readResponse(response: Response): Promise<unknown> {
  if (response.body === null) {
    throw new CreationError("Missing join information.", true);
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
      if (length > 4096) {
        throw new CreationError("The server returned invalid join information.", false);
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
      throw new CreationError("The server returned invalid join information.", false);
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
  const url = creationUrl(api);
  try {
    matchUrl(endpoint, "validation");
  } catch {
    throw new CreationError("Invalid game connection settings.", false);
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
      await response.body?.cancel();
      switch (response.status) {
        case 410:
          throw new CreationError(
            "This creation request has expired. Start a new game request.",
            false,
          );
        case 429:
          throw new CreationError("The server is full. You can retry this request.", true);
        case 503:
          throw new CreationError(
            "The server cannot create a game right now. You can retry this request.",
            true,
          );
        case 400:
          throw new CreationError("The creation request was rejected.", false);
        case 403:
          throw new CreationError(
            "This board is not allowed to create games on the server.",
            false,
          );
        default:
          throw new CreationError("Unable to create the game. You can retry this request.", true);
      }
    }
    return decodeJoinInformation(await readResponse(response), endpoint);
  } catch (error) {
    if (error instanceof CreationError) {
      throw error;
    }
    throw new CreationError("The response was interrupted. Retry to recover the same game.", true);
  }
}

export class JoinError extends Error {}

/** Public existence/lifetime check; never allocates a match or returns credentials. */
export async function lookupGame(
  api: string,
  endpoint: string,
  matchId: string,
  signal: AbortSignal,
): Promise<JoinInformation> {
  if (!joinInformation.shape.matchId.safeParse(matchId).success) {
    throw new JoinError("This game identifier is invalid. Ask the host for a new invitation.");
  }
  matchUrl(endpoint, matchId);
  const url = creationUrl(api);
  url.pathname += `/${matchId}`;
  const response = await fetch(url, {
    credentials: "omit",
    signal: AbortSignal.any([signal, AbortSignal.timeout(5000)]),
  });
  if (!response.ok) {
    await response.body?.cancel();
    if (response.status === 410) {
      throw new JoinError("This game has expired. Ask the host for a new invitation.");
    }
    if (response.status === 404) {
      throw new JoinError("This game is no longer available. Ask the host for a new invitation.");
    }
    throw new JoinError("Unable to check this game. Check your connection and retry.");
  }
  const join = decodeJoinInformation(await readResponse(response), endpoint);
  if (join.matchId !== matchId) {
    throw new JoinError("The server returned a different game.");
  }
  return join;
}
