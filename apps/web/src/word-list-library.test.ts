// Unit checks of the guest word-list client (#97): capabilities stay bound to the issuing
// API, an unstorable capability leaves no orphan, server feedback keeps its line number,
// and file import decodes strictly without sending anything.
import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import { canonicalApi } from "./layout-library";
import {
  createWordList,
  deleteWordList,
  importWordListFile,
  ownedWordLists,
  readWordList,
  WordListError,
  wordListShareUrl,
} from "./word-list-library";

const apiA = "http://127.0.0.1:8081";
const apiB = "http://localhost:9999/evil";
const capability = "c".repeat(64);
const wordListId = `w_${"a".repeat(32)}`;
const created = {
  version: 1,
  wordListId,
  ownerCapability: capability,
  name: "Mine",
  revision: 1,
  wordList: { name: "custom-words-x", revision: "1+word-profile-sha256:x" },
  wordCount: 2,
};

class MemoryStorage {
  values = new Map<string, string>();
  failWrites = false;
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
    if (this.failWrites) {
      throw new Error("QuotaExceededError");
    }
    this.values.set(key, value);
  }
  removeItem(key: string) {
    this.values.delete(key);
  }
}
let calls: { url: string; body: string }[] = [];
let storage: MemoryStorage;
let reply: () => Response;
const originalFetch = globalThis.fetch;
async function rejection(operation: Promise<unknown>): Promise<unknown> {
  try {
    await operation;
  } catch (error) {
    return error;
  }
  throw new Error("operation unexpectedly succeeded");
}

beforeEach(() => {
  calls = [];
  storage = new MemoryStorage();
  reply = () =>
    new Response(JSON.stringify(created), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  Object.defineProperty(globalThis, "localStorage", { value: storage, configurable: true });
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    let url: string;
    if (typeof input === "string") {
      url = input;
    } else if (input instanceof URL) {
      url = input.href;
    } else {
      url = input.url;
    }
    calls.push({ url, body: typeof init?.body === "string" ? init.body : "" });
    if (url.endsWith("/delete")) {
      return new Response(null, { status: 204 });
    }
    return reply();
  }) as typeof fetch;
});
afterEach(() => {
  globalThis.fetch = originalFetch;
});

describe("owner capabilities", () => {
  test("are stored per original and bound to the issuing API", async () => {
    await createWordList(apiA, "Mine", "CAT\nTA\n");
    const [entry] = ownedWordLists(apiA);
    expect(entry).toEqual({
      version: 1,
      wordListId,
      ownerCapability: capability,
      api: canonicalApi(apiA),
    });
    expect(storage.getItem(`scrabble:word-list-owner:v1:${wordListId}`)).not.toBeNull();
    expect(ownedWordLists(apiB)).toEqual([]);
    calls = [];
    for (const operation of [
      () => readWordList(apiB, entry!),
      () => deleteWordList(apiB, entry!),
    ]) {
      expect(await rejection(operation())).toBeInstanceOf(WordListError);
    }
    expect(calls).toEqual([]);
    await deleteWordList(apiA, entry!);
    expect(ownedWordLists(apiA)).toEqual([]);
  });
  test("an unstorable capability deletes the new original and fails", async () => {
    const setItem = storage.setItem.bind(storage);
    storage.setItem = (key: string, value: string) => {
      if (key.endsWith(wordListId)) {
        throw new Error("QuotaExceededError");
      }
      setItem(key, value);
    };
    expect(await rejection(createWordList(apiA, "Mine", "CAT\n"))).toMatchObject({
      code: "storage",
    });
    expect(calls.at(-1)?.url).toBe(`${apiA}/word-lists/${wordListId}/delete`);
    expect(ownedWordLists(apiA)).toEqual([]);
  });
  test("share links carry only the view and share id", () => {
    const previous = Object.getOwnPropertyDescriptor(globalThis, "window");
    Object.defineProperty(globalThis, "window", {
      value: { location: { href: "https://example.test/scrabble/?match=x&api=y" } },
      configurable: true,
    });
    try {
      const url = new URL(wordListShareUrl(`s_${"b".repeat(32)}`));
      expect([...url.searchParams.keys()].sort()).toEqual(["api", "share", "view"]);
      expect(url.searchParams.get("view")).toBe("word-list");
    } finally {
      if (previous) {
        Object.defineProperty(globalThis, "window", previous);
      } else {
        Reflect.deleteProperty(globalThis, "window");
      }
    }
  });
});

test("server rejections keep the offending line and nothing is stored", async () => {
  reply = () =>
    new Response(
      JSON.stringify({ error: "invalid-word-list", reason: "line 2 is not a word", line: 2 }),
      { status: 400 },
    );
  expect(await rejection(createWordList(apiA, "Mine", "CAT\nD0G\n"))).toMatchObject({
    code: "invalid",
    line: 2,
  });
  expect(ownedWordLists(apiA)).toEqual([]);
});

test("imports decode strictly as UTF-8 and respect the published byte bound", async () => {
  const limits = { version: 1 as const, maxBytes: 8, maxWords: 4 };
  expect(await importWordListFile(new Blob(["CAT\nTA\n"]), limits)).toEqual({
    kind: "text",
    text: "CAT\nTA\n",
  });
  expect(await importWordListFile(new Blob([new Uint8Array([0x43, 0xff, 0x0a])]), limits)).toEqual({
    kind: "notText",
  });
  expect(await importWordListFile(new Blob(["\n\n"]), limits)).toEqual({ kind: "empty" });
  expect(await importWordListFile(new Blob(["ABCDEFGHI"]), limits)).toEqual({
    kind: "tooLarge",
  });
  expect(calls).toEqual([]);
});
