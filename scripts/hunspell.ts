// Expands a Hunspell .aff/.dic pair into every word form its affix rules generate (#153).
// Compounding is not expanded: compound-only forms are dropped, so the result is the set
// of stand-alone forms. Supported directives: SET (UTF-8, ISO8859-1), FLAG (char, UTF-8,
// long, num), PFX/SFX with cross products and continuation classes (two levels),
// NEEDAFFIX, ONLYINCOMPOUND, FORBIDDENWORD and CIRCUMFIX. Morphological fields are ignored.

type Affix = {
  kind: "PFX" | "SFX";
  flag: string;
  cross: boolean;
  strip: string;
  add: string;
  continuation: Set<string>;
  condition: RegExp;
};
export type Affixes = {
  rules: Map<string, Affix[]>;
  parseFlags: (text: string) => string[];
  needAffix?: string;
  onlyInCompound?: string;
  forbidden?: string;
  circumfix?: string;
};

export function decode(bytes: Uint8Array, encoding: string): string {
  const label = /^ISO8859-1$/i.test(encoding) ? "latin1" : encoding;
  return new TextDecoder(label).decode(bytes);
}
/** The SET encoding declared anywhere in an .aff file (defaults to ISO8859-1, as Hunspell
 * does); pt_BR declares it after a long comment header. */
export function affixEncoding(bytes: Uint8Array): string {
  const text = new TextDecoder("latin1").decode(bytes);
  return /^SET\s+(\S+)/m.exec(text)?.[1] ?? "ISO8859-1";
}

function flagParser(type: string | undefined): (text: string) => string[] {
  if (type === "long") {
    return (text) => text.match(/../gsu) ?? [];
  }
  if (type === "num") {
    return (text) => text.split(",").filter((part) => part !== "");
  }
  return (text) => Array.from(text);
}
function condition(kind: "PFX" | "SFX", text: string): RegExp {
  if (text === ".") {
    return /^/u;
  }
  const escaped = text.replaceAll(/[\\^$*+?(){}|]/gu, (match) =>
    match === "^" ? match : `\\${match}`,
  );
  return new RegExp(kind === "PFX" ? `^${escaped}` : `${escaped}$`, "u");
}

export function parseAffixes(text: string): Affixes {
  const lines = text.split(/\r?\n/u);
  const flagType = /^FLAG\s+(\S+)/mu.exec(text)?.[1];
  const parseFlags = flagParser(flagType);
  const single = (name: string) => {
    const value = new RegExp(`^${name}\\s+(\\S+)`, "mu").exec(text)?.[1];
    return value === undefined ? undefined : parseFlags(value)[0];
  };
  const rules = new Map<string, Affix[]>();
  const crossByFlag = new Map<string, boolean>();
  for (const line of lines) {
    const fields = line.trim().split(/\s+/u);
    const kind = fields[0];
    if (kind !== "PFX" && kind !== "SFX") {
      continue;
    }
    const flag = fields[1]!;
    if (fields.length === 4 && /^\d+$/u.test(fields[3]!)) {
      crossByFlag.set(`${kind}:${flag}`, fields[2] === "Y");
      continue;
    }
    if (fields.length < 5) {
      continue;
    }
    const [addText, continuationText] = fields[3]!.split("/", 2) as [string, string | undefined];
    const affix: Affix = {
      kind,
      flag,
      cross: crossByFlag.get(`${kind}:${flag}`) ?? false,
      strip: fields[2] === "0" ? "" : fields[2]!,
      add: addText === "0" ? "" : addText,
      continuation: new Set(continuationText ? parseFlags(continuationText) : []),
      condition: condition(kind, fields[4]!),
    };
    const key = `${kind}:${flag}`;
    rules.set(key, [...(rules.get(key) ?? []), affix]);
  }
  return {
    rules,
    parseFlags,
    ...optional("needAffix", single("NEEDAFFIX")),
    ...optional("onlyInCompound", single("ONLYINCOMPOUND")),
    ...optional("forbidden", single("FORBIDDENWORD")),
    ...optional("circumfix", single("CIRCUMFIX")),
  };
}
function optional<K extends string>(key: K, value: string | undefined) {
  return (value === undefined ? {} : { [key]: value }) as Partial<Record<K, string>>;
}

function apply(affix: Affix, word: string): string | undefined {
  if (affix.kind === "SFX") {
    if (!affix.condition.test(word) || !word.endsWith(affix.strip)) {
      return undefined;
    }
    const stem = word.slice(0, word.length - affix.strip.length);
    return stem === "" && affix.add === "" ? undefined : stem + affix.add;
  }
  if (!affix.condition.test(word) || !word.startsWith(affix.strip)) {
    return undefined;
  }
  const stem = word.slice(affix.strip.length);
  return stem === "" && affix.add === "" ? undefined : affix.add + stem;
}

/** Every stand-alone form of one dictionary entry. */
export function expand(affixes: Affixes, word: string, flags: string[]): string[] {
  const own = new Set(flags);
  if (affixes.forbidden && own.has(affixes.forbidden)) {
    return [];
  }
  const out = new Set<string>();
  const blocked = (set: Set<string>) =>
    (affixes.onlyInCompound !== undefined && set.has(affixes.onlyInCompound)) ||
    (affixes.forbidden !== undefined && set.has(affixes.forbidden));
  const needs = (set: Set<string>) => affixes.needAffix !== undefined && set.has(affixes.needAffix);
  const circum = (affix: Affix) =>
    affixes.circumfix !== undefined && affix.continuation.has(affixes.circumfix);
  if (!needs(own) && !blocked(own)) {
    out.add(word);
  }
  const of = (kind: "PFX" | "SFX", set: Iterable<string>) =>
    [...set].flatMap((flag) => affixes.rules.get(`${kind}:${flag}`) ?? []);

  // Suffixes, optionally followed by one continuation suffix (twofold suffixes).
  const suffixed: { form: string; affix: Affix; outer?: Affix }[] = [];
  for (const first of of("SFX", own)) {
    const form = apply(first, word);
    if (form === undefined || blocked(first.continuation)) {
      continue;
    }
    suffixed.push({ form, affix: first });
    for (const second of of("SFX", first.continuation)) {
      const outer = apply(second, form);
      if (outer !== undefined && !blocked(second.continuation)) {
        suffixed.push({ form: outer, affix: first, outer: second });
      }
    }
  }
  for (const entry of suffixed) {
    const last = entry.outer ?? entry.affix;
    const standalone = entry.outer !== undefined || !needs(entry.affix.continuation);
    if (standalone && !needs(last.continuation) && !circum(entry.affix)) {
      out.add(entry.form);
    }
  }
  // Prefixes alone and cross products with cross-capable suffixes.
  for (const prefix of of("PFX", own)) {
    if (blocked(prefix.continuation)) {
      continue;
    }
    const alone = apply(prefix, word);
    if (alone !== undefined && !needs(prefix.continuation) && !circum(prefix)) {
      out.add(alone);
    }
    if (!prefix.cross) {
      continue;
    }
    for (const entry of suffixed) {
      if (!entry.affix.cross) {
        continue;
      }
      if (circum(prefix) !== circum(entry.affix)) {
        continue;
      }
      const both = apply(prefix, entry.form);
      if (both !== undefined) {
        out.add(both);
      }
    }
  }
  return [...out];
}

/** Parses .dic lines (the first line is the entry count; tab-separated morphology and
 * `#` comment lines are ignored) into word and flags. */
export function* entries(affixes: Affixes, text: string): Generator<[string, string[]]> {
  const lines = text.split(/\r?\n/u);
  for (const line of lines.slice(1)) {
    const entry = line.split("\t", 1)[0]!.trim();
    if (entry === "" || entry.startsWith("#")) {
      continue;
    }
    // A slash escaped as "\/" belongs to the word.
    const split = /^((?:\\\/|[^/])+)(?:\/(\S*))?/u.exec(entry.split(/\s/u, 1)[0]!);
    if (!split) {
      continue;
    }
    yield [split[1]!.replaceAll("\\/", "/"), split[2] ? affixes.parseFlags(split[2]) : []];
  }
}
