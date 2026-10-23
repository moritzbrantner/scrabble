import { expect, test } from "bun:test";
import { gzipSync } from "node:zlib";

import { enable2kWords, verifiedEnable2kWords, wordListDifference } from "./enable2k";

test("ENABLE2K preparation retains short valid cross-words and normalizes deterministically", () => {
  expect(enable2kWords("job\r\nod\nOD\nfoo-bar\na\nabcdefghijklmnop\njob\n")).toEqual([
    "JOB",
    "OD",
  ]);
  expect(enable2kWords("od\njob\n")).toEqual(["JOB", "OD"]);
});

test("ENABLE2K follows the existing ASCII English alphabet and inclusive 2..15 limits", () => {
  expect(
    enable2kWords(
      "aa\nabcdefghijklmno\nabcdefghijklmnop\na\ncafé\ncan't\nco-op\na1\n word\nword \nWord\n",
    ),
  ).toEqual(["AA", "ABCDEFGHIJKLMNO"]);
});

test("ENABLE2K rejects substituted or corrupted source bytes before using any words", () => {
  for (const bytes of [new Uint8Array(), gzipSync("job\nod\n"), new Uint8Array([31, 139, 0])]) {
    expect(() => verifiedEnable2kWords(bytes)).toThrow("differs from the reviewed source bytes");
  }
});

test("coverage report distinguishes newly playable from lost words", () => {
  expect(wordListDifference(["AT", "JOB"], ["JOB", "OD"])).toEqual({
    added: ["OD"],
    missing: ["AT"],
  });
});
