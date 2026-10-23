// Prepares expanded English, English (UK), German and Spanish word-list presets and their tile profiles
// (#153) as a profile catalog that extends the server's built-in profiles. Run by the
// operator at deploy time; the generated lists are never committed. Every list comes from
// a pinned free source, is checked against its reviewed count and hash, and keeps its
// licence and source notices next to it.
//
// bun scripts/prepare-word-lists.ts SCOWL_CHECKOUT LIBREOFFICE_DICTIONARIES_CHECKOUT ENABLE2K_CHECKOUT NEW_DIRECTORY
import { copyFile, mkdir, readFile, realpath } from "node:fs/promises";
import { resolve } from "node:path";

import { enable2kSource, verifiedEnable2kWords, wordListDifference } from "./enable2k";
import { affixEncoding, decode, entries, expand, parseAffixes } from "./hunspell";

const scowlRevision = "7e99edab8e32f9f9ea2b15f249ca8d4d67237410";
const dictionariesRevision = "67032ee957631908098a72edf48dc81cac27e369";
const generalSpelling = "General spelling — not official tournament lexicon";
const [scowlArgument, dictionariesArgument, enable2kArgument, destinationArgument] =
  Bun.argv.slice(2);
if (!scowlArgument || !dictionariesArgument || !enable2kArgument || !destinationArgument) {
  throw new Error(
    "Provide pinned SCOWL, LibreOffice dictionaries and ENABLE2K checkouts and a new output directory",
  );
}
const scowl = await realpath(scowlArgument);
const dictionaries = await realpath(dictionariesArgument);
const enable2k = await realpath(enable2kArgument);
const destination = resolve(destinationArgument);

async function run(cwd: string, command: string[]): Promise<string> {
  const child = Bun.spawn(command, { cwd, stdout: "pipe", stderr: "pipe" });
  const [output, errors, code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  if (code !== 0) {
    throw new Error(`Word-list preparation failed (${code}): ${command.join(" ")}\n${errors}`);
  }
  return output;
}
async function pinned(cwd: string, revision: string, paths: string[] = []) {
  if ((await run(cwd, ["git", "rev-parse", "HEAD"])).trim() !== revision) {
    throw new Error(`${cwd} must be checked out at the reviewed revision ${revision}`);
  }
  await run(cwd, ["git", "diff", "--quiet", "HEAD", "--", ...paths]);
}
await pinned(scowl, scowlRevision);
await pinned(dictionaries, dictionariesRevision, ["de", "es"]);
await pinned(enable2k, enable2kSource.revision);

const sha256 = (text: string | Uint8Array) =>
  new Bun.CryptoHasher("sha256").update(text).digest("hex");
const playable = (word: string, letters: RegExp) => {
  const length = Array.from(word).length;
  return length >= 2 && length <= 15 && letters.test(word);
};
const sorted = (words: Iterable<string>) => [...new Set(words)].sort();

/** SCOWL recipe: A for the built-in US baseline, B for British -ise spelling. */
async function english(variant: "A" | "B"): Promise<string[]> {
  const raw = await run(scowl, [
    "./scowl",
    "word-list",
    "60",
    variant,
    "1",
    "--wo-poses=abbr",
    "--categories=",
  ]);
  return sorted(
    raw
      .split(/\r?\n/u)
      .filter((word) => /^[a-z]{2,15}$/u.test(word))
      .map((word) => word.toUpperCase()),
  );
}
async function expandedEnglish(): Promise<string[]> {
  const archive = new Uint8Array(await readFile(resolve(enable2k, enable2kSource.file)));
  return verifiedEnable2kWords(archive);
}
async function hunspellForms(name: string): Promise<string[]> {
  const affBytes = new Uint8Array(
    await Bun.file(resolve(dictionaries, `${name}.aff`)).arrayBuffer(),
  );
  const encoding = affixEncoding(affBytes);
  const affixes = parseAffixes(decode(affBytes, encoding));
  const dic = decode(
    new Uint8Array(await Bun.file(resolve(dictionaries, `${name}.dic`)).arrayBuffer()),
    encoding,
  );
  const forms: string[] = [];
  for (const [word, flags] of entries(affixes, dic)) {
    // Abbreviations are written in capitals or with a full stop; neither is playable.
    if (word.includes(".") || /^\p{Lu}{2,}$/u.test(word)) {
      continue;
    }
    for (const form of expand(affixes, word, flags)) {
      forms.push(form);
    }
  }
  return forms;
}
/** German: Ä, Ö and Ü are their own letters and ß is written SS. igerman98 marks no proper
 * names, and German nouns are capitalized, so capitalized entries are kept. */
async function german(): Promise<string[]> {
  const forms = await hunspellForms("de/de_DE_frami");
  return sorted(
    forms
      .map((form) => form.toLocaleUpperCase("de-DE"))
      .filter((word) => playable(word, /^[A-ZÄÖÜ]+$/u)),
  );
}
/** Spanish: Ñ is its own letter, accents and diaeresis are dropped, capitalized entries
 * (proper names) are excluded, and only words spellable with Spanish tiles (no K or W)
 * remain. CH, LL and RR are spelled with their letters; the tiles add two-letter faces. */
async function spanish(): Promise<string[]> {
  const forms = await hunspellForms("es/es_ES");
  return sorted(
    forms
      .filter((form) => !/^\p{Lu}/u.test(form))
      .map((form) =>
        form
          .toLocaleUpperCase("es-ES")
          .replaceAll("Ñ", "\u0000")
          .normalize("NFD")
          .replaceAll(/\p{M}/gu, "")
          .replaceAll("\u0000", "Ñ"),
      )
      .filter((word) => playable(word, /^[A-JL-VX-ZÑ]+$/u)),
  );
}

// Standard published German (102 tiles) and Spanish (100 tiles, with CH, LL and RR)
// distributions, shared with the server's catalog tests.
const tileProfiles: unknown = await Bun.file(
  resolve(import.meta.dir, "preset-tile-profiles.json"),
).json();
await run(scowl, ["make", "-B"]);
const scowlUs = await english("A");
const scowlUk = await english("B");
const enable2kEnglish = await expandedEnglish();
const coverage = wordListDifference(scowlUs, enable2kEnglish);
console.log(
  `ENABLE2K vs SCOWL en-US: ${coverage.added.length} additional, ${coverage.missing.length} absent; ` +
    `${enable2kEnglish.filter((word) => word.length === 2).length} two-letter words`,
);
if (!enable2kEnglish.includes("OD") || !enable2kEnglish.includes("JOB")) {
  throw new Error("ENABLE2K lacks the reviewed OD/JOB cross-word regression");
}

const lists = [
  {
    file: "words-enable2k-en.txt",
    identity: { name: "enable2k-en", revision: "2000-recipe1" },
    label: "English (expanded) — ENABLE2K 2000, public domain (not official tournament lexicon)",
    words: enable2kEnglish,
    source: "https://github.com/BartMassey/wordlists",
    sourceRevision: enable2kSource.revision,
    sourceFile: enable2kSource.file,
    sourceSha256: enable2kSource.sha256,
    originators: "Alan Beale and M. Cooper",
    usage:
      "Broad English word-game list, millennial edition (2000); not an official tournament lexicon",
    recipe: "enable2k.txt.gz; lowercase ASCII 2..15; uppercase; deduplicate; sort",
    notices: [[resolve(enable2k, "README-enable2k.txt"), "ENABLE2K-README.txt"]],
  },
  {
    file: "words-en-gb.txt",
    identity: { name: "scowl-en-gb-60", revision: "2026.02.25-recipe1" },
    label: `English (UK) — SCOWL size 60, 2026.02.25 (${generalSpelling})`,
    words: scowlUk,
    source: "https://github.com/en-wl/wordlist",
    sourceRevision: scowlRevision,
    recipe:
      "./scowl word-list 60 B 1 --wo-poses=abbr --categories=; lowercase ASCII 2..15; uppercase",
    notices: [[resolve(scowl, "Copyright"), "SCOWL-Copyright"]],
  },
  {
    file: "words-de.txt",
    identity: { name: "igerman98-de-de-frami", revision: "2017.01.12-recipe1" },
    label: `Deutsch — igerman98 (frami), 2017.01.12 (${generalSpelling})`,
    words: await german(),
    source: "https://github.com/LibreOffice/dictionaries/tree/master/de",
    sourceRevision: dictionariesRevision,
    recipe:
      "de_DE_frami Hunspell affix expansion without compounds; drop abbreviations; uppercase (ß as SS); letters A-Z ÄÖÜ, length 2..15",
    notices: [
      [resolve(dictionaries, "de/COPYING_GPLv2"), "igerman98-COPYING_GPLv2"],
      [resolve(dictionaries, "de/COPYING_GPLv3"), "igerman98-COPYING_GPLv3"],
      [resolve(dictionaries, "de/README_de_DE_frami.txt"), "igerman98-README_de_DE_frami.txt"],
    ],
  },
  {
    file: "words-es.txt",
    identity: { name: "rla-es-es-es", revision: "2.9-recipe1" },
    label: `Español (España) — RLA-ES 2.9 (${generalSpelling})`,
    words: await spanish(),
    source: "https://github.com/LibreOffice/dictionaries/tree/master/es",
    sourceRevision: dictionariesRevision,
    recipe:
      "es_ES Hunspell affix expansion; drop capitalized entries and abbreviations; uppercase; drop accents and diaeresis except Ñ; letters A-Z without K and W plus Ñ, length 2..15",
    notices: [
      [resolve(dictionaries, "es/LICENSE.md"), "RLA-ES-LICENSE.md"],
      [resolve(dictionaries, "es/GPLv3.txt"), "RLA-ES-GPLv3.txt"],
      [resolve(dictionaries, "es/LGPLv3.txt"), "RLA-ES-LGPLv3.txt"],
      [resolve(dictionaries, "es/MPL-1.1.txt"), "RLA-ES-MPL-1.1.txt"],
      [resolve(dictionaries, "es/README_hunspell_es.txt"), "RLA-ES-README_hunspell_es.txt"],
    ],
  },
];
const reviewed: Record<string, { count: number; sha256: string }> = {
  "enable2k-en": {
    count: 169266,
    sha256: "452528054b24cce68199a01534e4639d451ac0da3bdac70113546ad6e13fda0e",
  },
  "scowl-en-gb-60": {
    count: 78021,
    sha256: "1acea2a1b62a834818754d4352e675c5b0d3bf73f2d673812c225eb62fdd34fd",
  },
  "igerman98-de-de-frami": {
    count: 540287,
    sha256: "05f5a84d1640c52650569d1d18a286219c7649160fc6142075c9e00ad5e2c70d",
  },
  "rla-es-es-es": {
    count: 580715,
    sha256: "8841e3022de6dd5c3ecef4689e0c20568716b1d5226ad51cbf491ea7d47243d9",
  },
};
const prepared = lists.map((list) => {
  const content = `${list.words.join("\n")}\n`;
  return { ...list, content, count: list.words.length, sha256: sha256(content) };
});
const mismatched = prepared.filter(
  (list) =>
    reviewed[list.identity.name]?.count !== list.count ||
    reviewed[list.identity.name]?.sha256 !== list.sha256,
);
if (mismatched.length > 0) {
  for (const list of mismatched) {
    console.error(`${list.identity.name}: ${list.count} words, sha256 ${list.sha256}`);
  }
  throw new Error("Word-list output differs from the reviewed recipe");
}

// Refuse an existing directory so deployed content and recovery inputs are never overwritten.
await mkdir(destination, { mode: 0o700 });
for (const list of prepared) {
  await Bun.write(resolve(destination, list.file), list.content);
  for (const [from, to] of list.notices) {
    await copyFile(from!, resolve(destination, to!));
  }
}
await Bun.write(
  resolve(destination, "catalog.json"),
  `${JSON.stringify(
    {
      version: 1,
      includeBuiltin: true,
      builtinWordListLabel: `English (US) — SCOWL size 60, 2026.02.25 (${generalSpelling})`,
      wordLists: prepared.map((list) => ({
        identity: list.identity,
        label: list.label,
        file: list.file,
      })),
      tileProfiles,
    },
    null,
    2,
  )}\n`,
);
await Bun.write(
  resolve(destination, "provenance.json"),
  `${JSON.stringify(
    {
      version: 1,
      wordLists: prepared.map((list) => ({
        identity: list.identity,
        file: list.file,
        source: list.source,
        sourceRevision: list.sourceRevision,
        sourceFile: list.sourceFile,
        sourceSha256: list.sourceSha256,
        originators: list.originators,
        recipe: list.recipe,
        wordCount: list.count,
        sha256: list.sha256,
        notices: list.notices.map(([, to]) => to),
        usage: list.usage ?? generalSpelling,
      })),
      tileSources:
        "Standard published German and Spanish Scrabble letter distributions (counts and values)",
    },
    null,
    2,
  )}\n`,
);
console.log(prepared.map((list) => `${list.identity.name}: ${list.count} words`).join("\n"));
