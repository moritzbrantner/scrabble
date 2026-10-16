// Regression tests for the Codex review of #133: owner capabilities stay bound to the
// issuing API, and a capability that cannot be stored leaves no orphaned original.
import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import {
  canonicalApi,
  copyShared,
  createLayout,
  deleteLayout,
  LayoutError,
  ownedEntries,
  readLayout,
} from "./layout-library";

const apiA = "http://127.0.0.1:8081";
const apiB = "http://localhost:9999/evil";
const layout = { boardShape: "square" as const, boardSize: 5, premiums: Array(25).fill("normal") };
const capability = "c".repeat(64);
const layoutId = `l_${"a".repeat(32)}`;
const created = {
  version: 1,
  layoutId,
  ownerCapability: capability,
  name: "Mine",
  revision: 1,
  board: { name: "custom-x", revision: "1+board" },
  layout,
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
type Call = { url: string; body: string };
let calls: Call[] = [];
let storage: MemoryStorage;
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
    return new Response(JSON.stringify(created), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }) as typeof fetch;
});
afterEach(() => {
  globalThis.fetch = originalFetch;
});

describe("owner capabilities are bound to their issuing API", () => {
  test("an entry issued by API A is never listed for or sent to API B", async () => {
    await createLayout(apiA, "Mine", layout);
    const [entry] = ownedEntries(apiA);
    expect(entry?.api).toBe(canonicalApi(apiA));
    expect(ownedEntries(apiB)).toEqual([]);
    calls = [];
    for (const operation of [() => readLayout(apiB, entry!), () => deleteLayout(apiB, entry!)]) {
      expect(await rejection(operation())).toBeInstanceOf(LayoutError);
    }
    expect(calls).toEqual([]);
    // The foreign API never causes local deletion either.
    expect(ownedEntries(apiA)).toHaveLength(1);
  });
  test("legacy entries without an issuing API are not offered to a development API", () => {
    storage.setItem(
      `scrabble:layout-owner:v1:${layoutId}`,
      JSON.stringify({ version: 1, layoutId, ownerCapability: capability }),
    );
    expect(ownedEntries(apiA)).toEqual([]);
    expect(ownedEntries(apiB)).toEqual([]);
  });
});

describe("a capability that cannot be stored leaves no orphan", () => {
  test("unavailable storage fails before the server creates anything", async () => {
    storage.failWrites = true;
    expect(await rejection(createLayout(apiA, "Mine", layout))).toMatchObject({ code: "storage" });
    expect(calls).toEqual([]);
  });
  for (const [label, run] of [
    ["create", () => createLayout(apiA, "Mine", layout)],
    ["copy", () => copyShared(apiA, `s_${"b".repeat(32)}`, "Mine")],
  ] as const) {
    test(`a failed store after ${label} deletes the new original with the in-memory capability`, async () => {
      const setItem = storage.setItem.bind(storage);
      storage.setItem = (key: string, value: string) => {
        if (key.endsWith(layoutId)) {
          throw new Error("QuotaExceededError");
        }
        setItem(key, value);
      };
      expect(await rejection(run())).toMatchObject({ code: "storage" });
      const compensation = calls.at(-1)!;
      expect(compensation.url).toBe(`${apiA}/layouts/${layoutId}/delete`);
      expect(JSON.parse(compensation.body)).toEqual({ version: 1, ownerCapability: capability });
      expect(ownedEntries(apiA)).toEqual([]);
    });
  }
});
