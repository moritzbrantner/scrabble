import { expect, test } from "bun:test";

import { entries, expand, parseAffixes } from "./hunspell";

const aff = `SET UTF-8
NEEDAFFIX h
ONLYINCOMPOUND o
FORBIDDENWORD d
CIRCUMFIX f
PFX U Y 1
PFX U 0 un .
SFX S Y 2
SFX S 0 s [^s]
SFX S 0 es s
SFX E N 1
SFX E 0 er/T .
SFX T N 1
SFX T 0 in .
SFX X Y 1
SFX X 0 lich/h .
SFX L N 1
SFX L 0 keit .
SFX C Y 1
SFX C 0 st/f .
PFX G Y 1
PFX G 0 ge/f .
SFX O N 1
SFX O 0 zeug/o .
`;
const dic = `7
fair/US
bus/S
lehr/E
freund/X
glücklich/hL
ruf/CG
hand/O
bad/d
`;

test("affix expansion covers cross products, two-level suffixes and special flags", () => {
  const affixes = parseAffixes(aff);
  const words = new Set(
    [...entries(affixes, dic)].flatMap(([word, flags]) => expand(affixes, word, flags)),
  );
  expect([...words].sort()).toEqual([
    "bus",
    "buses",
    "fair",
    "fairs",
    "freund",
    "gerufst",
    "glücklichkeit",
    "hand",
    "lehr",
    "lehrer",
    "lehrerin",
    "ruf",
    "unfair",
    "unfairs",
  ]);
});
