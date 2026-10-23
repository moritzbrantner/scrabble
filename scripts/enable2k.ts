import { gunzipSync } from "node:zlib";

/** Normalize ENABLE2K's public-domain word-game corpus to English Scrabble tiles. */
export function enable2kWords(source: string): string[] {
  return [
    ...new Set(
      source
        .split(/\r?\n/u)
        .filter((word) => /^[a-z]{2,15}$/u.test(word))
        .map((word) => word.toUpperCase()),
    ),
  ].sort();
}

export function wordListDifference(base: readonly string[], candidate: readonly string[]) {
  const baseline = new Set(base);
  const expanded = new Set(candidate);
  return {
    added: candidate.filter((word) => !baseline.has(word)),
    missing: base.filter((word) => !expanded.has(word)),
  };
}
export const enable2kSource = {
  revision: "af52415c13af809bd8757a40f17f46e79d09583c",
  file: "enable2k.txt.gz",
  sha256: "2c1093669cd16439bdb0a693a0058626c9c9f82e59244c9b0bde89515d44d3ad",
};

/** Verify the original compressed source before decompressing or normalizing it. */
export function verifiedEnable2kWords(archive: Uint8Array): string[] {
  if (new Bun.CryptoHasher("sha256").update(archive).digest("hex") !== enable2kSource.sha256) {
    throw new Error("ENABLE2K archive differs from the reviewed source bytes");
  }
  return enable2kWords(gunzipSync(archive).toString("utf8"));
}
