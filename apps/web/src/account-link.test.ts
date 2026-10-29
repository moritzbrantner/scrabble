// Account linking client (#132): links every library this browser owns and stores device
// capabilities for linked libraries it does not hold yet. A fake game API answers.
import { afterEach, beforeEach, expect, test } from "bun:test";

import {
  AccountError,
  accountSession,
  linkLocalLibraries,
  localOwners,
  reconnectLinkedLibraries,
} from "./account-link";
import { canonicalApi } from "./layout-library";

const api = "http://127.0.0.1:8081";
const wordListId = `w_${"a".repeat(32)}`;
const profileId = `t_${"b".repeat(32)}`;
const layoutId = `l_${"c".repeat(32)}`;

class MemoryStorage {
  values = new Map<string, string>();
  get length() {
    return this.values.size;
  }
  key(index: number) {
    return [...this.values.keys()][index] ?? null;
  }
  getItem(key: string) {
    return this.values.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.values.set(key, value);
  }
  removeItem(key: string) {
    this.values.delete(key);
  }
}
type Call = { path: string; body: Record<string, unknown>; credentials?: RequestCredentials };
let calls: Call[] = [];
let answer: (path: string, body: Record<string, unknown>) => [number, unknown];
const originalFetch = globalThis.fetch;

beforeEach(() => {
  calls = [];
  Object.defineProperty(globalThis, "localStorage", {
    value: new MemoryStorage(),
    configurable: true,
  });
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = new URL(input instanceof URL ? input.href : (input as string)).pathname;
    const body = JSON.parse(init?.body as string) as Record<string, unknown>;
    calls.push({ path, body, ...(init?.credentials ? { credentials: init.credentials } : {}) });
    const [status, payload] = answer(path, body);
    return new Response(JSON.stringify(payload), {
      status,
      headers: { "Content-Type": "application/json" },
    });
  }) as typeof fetch;
});
afterEach(() => {
  globalThis.fetch = originalFetch;
});

function own(prefix: string, entry: Record<string, unknown>, id: string) {
  localStorage.setItem(
    prefix + id,
    JSON.stringify({
      version: 1,
      ownerCapability: "0".repeat(64),
      api: canonicalApi(api),
      ...entry,
    }),
  );
}

test("a signed-out answer carries the account page and the cookie is sent", async () => {
  answer = () => [401, { error: "signed-out", signIn: "https://moenarch.com/accounts/" }];
  const error = await accountSession(api).catch((caught: unknown) => caught);
  expect(error).toBeInstanceOf(AccountError);
  expect((error as AccountError).code).toBe("signedOut");
  expect((error as AccountError).signIn).toBe("https://moenarch.com/accounts/");
  expect(calls[0]).toEqual({
    path: "/account/session",
    body: { version: 1 },
    credentials: "include",
  });
});

test("a guest-only board origin answers like a server without accounts", async () => {
  answer = () => [403, { error: "guest-origin" }];
  const error = (await accountSession(api).catch((caught: unknown) => caught)) as AccountError;
  expect(error.code).toBe("unavailable");
});

test("an unsafe sign-in link is not offered", async () => {
  answer = () => [401, { error: "signed-out", signIn: "javascript:alert(1)" }];
  const error = (await accountSession(api).catch((caught: unknown) => caught)) as AccountError;
  expect(error.signIn).toBeUndefined();
});

test("all three library kinds link; conflicts are counted, not fatal", async () => {
  own("scrabble:word-list-owner:v1:", { wordListId }, wordListId);
  own("scrabble:tile-profile-owner:v1:", { profileId }, profileId);
  own("scrabble:layout-owner:v1:", { layoutId }, layoutId);
  answer = (_path, body) =>
    body.kind === "layout"
      ? [409, { error: "account-link-conflict" }]
      : [200, { linked: body.kind === "wordList" ? "new" : "existing" }];
  expect(await linkLocalLibraries(api)).toEqual({ linked: 1, existing: 1, conflicts: 1 });
  expect(calls.map((call) => [call.path, call.body.kind, call.body.id])).toEqual([
    ["/account/link", "wordList", wordListId],
    ["/account/link", "tileProfile", profileId],
    ["/account/link", "layout", layoutId],
  ]);
});

test("reconnect stores device capabilities only for libraries not held here", async () => {
  own("scrabble:word-list-owner:v1:", { wordListId }, wordListId);
  answer = (path, body) =>
    path === "/account/libraries"
      ? [
          200,
          {
            version: 1,
            wordLists: [{ id: wordListId, name: "Held" }],
            tileProfiles: [{ id: profileId, name: "Tiles" }],
            layouts: [{ id: layoutId, name: "Board" }],
          },
        ]
      : [200, { version: 1, kind: body.kind, id: body.id, ownerCapability: "f".repeat(64) }];
  expect(await reconnectLinkedLibraries(api)).toBe(2);
  expect(
    calls.filter((call) => call.path === "/account/reconnect").map((call) => call.body.id),
  ).toEqual([profileId, layoutId]);
  expect(localOwners(api).map((owner) => [owner.kind, owner.id, owner.ownerCapability])).toEqual([
    ["wordList", wordListId, "0".repeat(64)],
    ["tileProfile", profileId, "f".repeat(64)],
    ["layout", layoutId, "f".repeat(64)],
  ]);
});
