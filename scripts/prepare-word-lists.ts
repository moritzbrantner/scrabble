// Prepares expanded English, English (UK), German and Spanish (#153) plus French, Italian,
// Portuguese (Portugal and Brazil), Dutch and Polish (#216) word-list presets and their tile
// profiles as a profile catalog that extends the server's built-in profiles. Run by the
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
await pinned(dictionaries, dictionariesRevision, [
  "de",
  "es",
  "fr_FR",
  "it_IT",
  "pt_PT",
  "pt_BR",
  "nl_NL",
  "pl_PL",
]);
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
/** Every expanded form passed through `keep`; Italian alone expands to tens of millions of
 * raw forms, so they are filtered as they are generated. */
async function hunspellForms(
  name: string,
  keep: (form: string) => string | undefined = (form) => form,
): Promise<string[]> {
  const affBytes = new Uint8Array(
    await Bun.file(resolve(dictionaries, `${name}.aff`)).arrayBuffer(),
  );
  const encoding = affixEncoding(affBytes);
  const affixes = parseAffixes(decode(affBytes, encoding));
  const dic = decode(
    new Uint8Array(await Bun.file(resolve(dictionaries, `${name}.dic`)).arrayBuffer()),
    encoding,
  );
  const forms = new Set<string>();
  for (const [word, flags] of entries(affixes, dic)) {
    // Abbreviations are written in capitals or with a full stop; neither is playable.
    if (word.includes(".") || /^\p{Lu}{2,}$/u.test(word)) {
      continue;
    }
    for (const form of expand(affixes, word, flags)) {
      const kept = keep(form);
      if (kept !== undefined) {
        forms.add(kept);
      }
    }
  }
  return [...forms];
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

/** Uppercases a form and drops accents except on the language's own tile letters. */
function tileSpelling(form: string, locale: string, ownLetters: string): string {
  let out = "";
  for (const letter of form.toLocaleUpperCase(locale).normalize("NFC")) {
    out += ownLetters.includes(letter) ? letter : letter.normalize("NFD").replaceAll(/\p{M}/gu, "");
  }
  return out;
}
/** Shared recipe for the #216 languages: expand, drop capitalized entries (proper names) and
 * abbreviations, spell with the language's tiles, keep playable words. */
async function language(
  name: string,
  locale: string,
  ownLetters: string,
  letters: RegExp,
  rewrite: (word: string) => string = (word) => word,
): Promise<string[]> {
  return sorted(
    await hunspellForms(name, (form) => {
      if (/^\p{Lu}/u.test(form)) {
        return undefined;
      }
      const word = rewrite(tileSpelling(form, locale, ownLetters));
      return playable(word, letters) ? word : undefined;
    }),
  );
}
/** French: accents and cedilla are dropped, Œ/Æ are written OE/AE; elisions and hyphenated
 * forms are not playable. */
const french = () =>
  language("fr_FR/dictionaries/fr", "fr-FR", "", /^[A-Z]+$/u, (word) =>
    word.replaceAll("Œ", "OE").replaceAll("Æ", "AE"),
  );
/** Italian: accents are dropped and only the 21 Italian tile letters (no J, K, W, X, Y)
 * remain; elided forms (dell', ...) are not playable. */
const italian = () => language("it_IT/it_IT", "it-IT", "", /^[A-IL-VZ]+$/u);
/** Portuguese: Ç is its own tile, other accents are dropped; no K, W or Y tiles. */
const portuguese = (name: string, locale: string) => language(name, locale, "Ç", /^[A-JL-VXZÇ]+$/u);
/** Dutch: the IJ ligature is spelled I + J (two tiles, the current Dutch convention),
 * accents and diaeresis are dropped, and the apostrophe of plurals such as auto's is
 * omitted. */
const dutch = () =>
  language("nl_NL/nl_NL", "nl-NL", "", /^[A-Z]+$/u, (word) =>
    word.replaceAll("Ĳ", "IJ").replaceAll("'", ""),
  );
/** Polish: Ą Ć Ę Ł Ń Ó Ś Ź Ż are their own tiles; no Q, V or X tiles. */
const polish = () => language("pl_PL/pl_PL", "pl-PL", "ĄĆĘŁŃÓŚŹŻ", /^[A-PR-UWYZĄĆĘŁŃÓŚŹŻ]+$/u);

// Standard published German (102 tiles), Spanish (100 tiles, with CH, LL and RR), French
// (102), Italian (120), Portuguese (120, Brazilian set), Dutch (102) and Polish (100)
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
  {
    file: "words-fr.txt",
    identity: { name: "grammalecte-fr", revision: "7.7-recipe1" },
    label: `Français — Grammalecte 7.7 (${generalSpelling})`,
    words: await french(),
    source: "https://github.com/LibreOffice/dictionaries/tree/master/fr_FR",
    sourceRevision: dictionariesRevision,
    recipe:
      "fr Hunspell affix expansion; drop capitalized entries and abbreviations; uppercase; drop accents and cedilla; Œ/Æ as OE/AE; letters A-Z, length 2..15",
    notices: [
      [
        resolve(dictionaries, "fr_FR/dictionaries/README_dict_fr.txt"),
        "Grammalecte-README_dict_fr.txt",
      ],
    ],
  },
  {
    file: "words-it.txt",
    identity: { name: "libreitalia-it-it", revision: "2020.11.07-recipe1" },
    label: `Italiano — LibreItalia 2020.11.07 (${generalSpelling})`,
    words: await italian(),
    source: "https://github.com/LibreOffice/dictionaries/tree/master/it_IT",
    sourceRevision: dictionariesRevision,
    recipe:
      "it_IT Hunspell affix expansion; drop capitalized entries, abbreviations and elisions; uppercase; drop accents; letters A-Z without J K W X Y, length 2..15",
    notices: [[resolve(dictionaries, "it_IT/README_it_IT.txt"), "LibreItalia-README_it_IT.txt"]],
  },
  {
    file: "words-pt-pt.txt",
    identity: { name: "vero-pt-pt", revision: "20.10.5.1-recipe1" },
    label: `Português (Portugal) — 20.10.5.1 (${generalSpelling})`,
    words: await portuguese("pt_PT/pt_PT", "pt-PT"),
    source: "https://github.com/LibreOffice/dictionaries/tree/master/pt_PT",
    sourceRevision: dictionariesRevision,
    recipe:
      "pt_PT Hunspell affix expansion; drop capitalized entries, abbreviations and hyphenated forms; uppercase; drop accents except Ç; letters A-Z without K W Y plus Ç, length 2..15",
    notices: [
      [resolve(dictionaries, "pt_PT/LICENSES.txt"), "pt_PT-LICENSES.txt"],
      [resolve(dictionaries, "pt_PT/README_pt_PT.txt"), "pt_PT-README_pt_PT.txt"],
    ],
  },
  {
    file: "words-pt-br.txt",
    identity: { name: "vero-pt-br", revision: "3.3.0-recipe1" },
    label: `Português (Brasil) — VERO 3.3.0 (${generalSpelling})`,
    words: await portuguese("pt_BR/pt_BR", "pt-BR"),
    source: "https://github.com/LibreOffice/dictionaries/tree/master/pt_BR",
    sourceRevision: dictionariesRevision,
    recipe:
      "pt_BR Hunspell affix expansion; drop capitalized entries, abbreviations and hyphenated forms; uppercase; drop accents except Ç; letters A-Z without K W Y plus Ç, length 2..15",
    notices: [
      [resolve(dictionaries, "pt_BR/README_pt_BR.txt"), "VERO-README_pt_BR.txt"],
      [resolve(dictionaries, "pt_BR/README_en.txt"), "VERO-README_en.txt"],
    ],
  },
  {
    file: "words-nl.txt",
    identity: { name: "opentaal-nl", revision: "2.20.21-recipe1" },
    label: `Nederlands — OpenTaal 2.20.21 (${generalSpelling})`,
    words: await dutch(),
    source: "https://github.com/LibreOffice/dictionaries/tree/master/nl_NL",
    sourceRevision: dictionariesRevision,
    recipe:
      "nl_NL Hunspell affix expansion without compounds; drop capitalized entries and abbreviations; uppercase; IJ ligature as I+J; drop accents and apostrophes; letters A-Z, length 2..15",
    notices: [
      [resolve(dictionaries, "nl_NL/LICENSE.txt"), "OpenTaal-LICENSE.txt"],
      [resolve(dictionaries, "nl_NL/README.md"), "OpenTaal-README.md"],
    ],
  },
  {
    file: "words-pl.txt",
    identity: { name: "sjp-pl-pl", revision: "2017.05.14-recipe1" },
    label: `Polski — SJP.PL 2017.05.14 (${generalSpelling})`,
    words: await polish(),
    source: "https://github.com/LibreOffice/dictionaries/tree/master/pl_PL",
    sourceRevision: dictionariesRevision,
    recipe:
      "pl_PL Hunspell affix expansion; drop capitalized entries and abbreviations; uppercase; letters A-Z without Q V X plus Ą Ć Ę Ł Ń Ó Ś Ź Ż, length 2..15",
    notices: [
      [resolve(dictionaries, "pl_PL/README_pl_PL.txt"), "SJP-README_pl_PL.txt"],
      [resolve(dictionaries, "pl_PL/README_en.txt"), "SJP-README_en.txt"],
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
  "grammalecte-fr": {
    count: 401798,
    sha256: "efef28733509c7dcd3494873f3b26ec4424f4e8528173cb545a49599a00d44fe",
  },
  "libreitalia-it-it": {
    count: 2416618,
    sha256: "2fffcc871d132e7053a947328d1bf690598a304dff8255914637d27930585de6",
  },
  "vero-pt-pt": {
    count: 372432,
    sha256: "e637c5a0c3122292d0997f9cb098c899ada16b571281053742f52bc2637a2d1f",
  },
  "vero-pt-br": {
    count: 1069342,
    sha256: "f81bc281506f56008806ad7565a0781304621fd3247cd4e95afc30e54cbd99b3",
  },
  "opentaal-nl": {
    count: 249860,
    sha256: "b59a4852eb89ca1450e48880dad6144563ef1313cda3c1aec3b1935593503681",
  },
  "sjp-pl-pl": {
    count: 3367142,
    sha256: "63f37c212cfcaf864bed6790d02020b31ac5117dd31cfc21d45a5d37d138cf03",
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
        "Standard published German, Spanish, French, Italian, Portuguese (Brazilian set), Dutch and Polish Scrabble letter distributions (counts and values)",
    },
    null,
    2,
  )}\n`,
);
console.log(prepared.map((list) => `${list.identity.name}: ${list.count} words`).join("\n"));
