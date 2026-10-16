# Multi-letter tiles in the browser (#98)

This is the acceptance contract for the playable square-board slice of #98. It
follows Atlas Decisions 1.2 and 1.21 (multi-letter faces), 1.25 and 1.31
(independently selected word list, tile profile and board layout, with
authoritative compatibility checks). It was written before any #98 production
code, on `a60d72e`. That commit contains the headless engine enabler #116
([multi-letter-tiles.md](multi-letter-tiles.md)).

The native engine already shows that a CH tile takes one rack slot and one
cell, adds its whole face to every word it forms, scores its value once and
keeps blanks single-letter. This contract does not test those rules again. It
covers what #98 adds:

- a selectable sample profile;
- the browser schema;
- rack, draft, preview, board and history presentation;
- legibility;
- keyboard and touch input;
- accessibility;
- reconnect;
- the public journey.

The hexagonal `letters`/`tiles` main-word metric (Decision 1.23) belongs to
#96, not to this slice. Tile-profile authoring (#100) and spellability
(open question 1 of multi-letter-tiles.md) are also out of scope.

## Sample profile and compatible word list

### Production (public Pages server)

Atlas 1.41 subsequently chose the English CH/SCH sample. Issue #121 adds it to
the actual fallback catalog as described in [server-executable.md](server-executable.md#independent-predefined-match-setup),
with independent acceptance in [public-multi-letter-preset.md](public-multi-letter-preset.md).
The original contract and fixture chronology below remain historical acceptance;
implementation does not establish deployment or hosted gameplay.

The game server used by the public Pages client must offer, through
`GET /profiles`:

- a **tile profile** with at least the faces `CH` and `SCH`
  (`{"kind":"letters",…}`), ordinary single letters including `C`, `H` and
  `S`, and a blank;
- a **word list** that `compose_profiles` accepts with that profile and that
  contains `CHAT`.

The host selects these with the existing three selectors. Untouched defaults do
not change: an untouched deployment keeps `legacyCompatible: true` and its
version-2 creation. Selecting the sample uses version-3 creation.

At this contract's original #98 authoring baseline, the production server ran
without a catalog:
`deploy/hetzner.env.example` sets no `SCRABBLE_PROFILE_CATALOG_FILE`, so
`ProfileCatalog::load` used its built-in single-profile fallback. The sample
therefore had to come from one of two places, and **which one is an
implementation choice**:

- an **additive built-in entry** in that fallback. PR #118 uses the same pattern
  for built-in board sizes;
- an **operator catalog** that the deployment adds, as described in
  [server-executable.md](server-executable.md#independent-predefined-match-setup).

Routine option recommended by this contract: a built-in tile profile made of the
deployment's own tiles plus `CH` and `SCH`, paired with the **existing
deployment word list** (`scowl-en-us-60`, which contains CHAT, SCHOOL and
similar words). This needs no new public word list, and compatibility holds
because every face letter is already a single-letter face. The labels, counts
and values are routine. See open question 1.

Release order: a client from before #98 cannot parse a catalog that contains a
`letters` face. Its whole creation form then shows "profiles unavailable", which
the fixture confirmed on `a60d72e`. The server sample must therefore not go live
before the #98 frontend. The existing rule to release frontend and server
together applies.

### Browser fixture (test data, never a deployment default)

`crates/scrabble-server/examples/browser_game_fixture.rs` (`mod multi_letter`)
adds two catalog sources to the real fixture catalog:

| Source    | Identity                        | Label                               |
| --------- | ------------------------------- | ----------------------------------- |
| word list | `acceptance-multi-letter-words` | `Multi-letter test words`           |
| tiles     | `acceptance-multi-letter-tiles` | `Multi-letter test tiles (CH, SCH)` |

The word list contains exactly `CHAT` and `SCHACH`.

The tiles are 29 in total, which meets the default 4 players × 7 rack minimum:

| Face  | A   | E   | N   | S   | T   | C   | H   | CH  | SCH | blank |
| ----- | --- | --- | --- | --- | --- | --- | --- | --- | --- | ----- |
| count | 6   | 4   | 2   | 2   | 4   | 1   | 1   | 4   | 3   | 2     |
| value | 1   | 1   | 1   | 1   | 1   | 3   | 2   | 5   | 8   | 0     |

The board stays `Classic square 15`, and the rules stay the catalog defaults.

The fixture always offers these sources, so the default `bun run test:browser`
runs `multi-letter-tiles.spec.ts`. When the contract was written on `a60d72e`,
they were gated by `SCRABBLE_BROWSER_MULTI_LETTER=1`, because the client of that
time rejected the whole catalog and every existing browser suite would have
failed. The #98 implementation removes that gate. Without the sources, the spec
fails with an explicit "catalog must offer the multi-letter sample" message. It
is never skipped.

`browser_game_fixture --multi-letter-opening <dir>` finds a real version-3
creation input (`requestId`, `requestedAt`) whose native deal gives:

- Ada (seat 2): CH, A, T and a blank;
- Lin (seat 3): SCH and A.

The browser then uses that input through the existing `selectOpening`. Configured
`p_` initialization is private to the crate, so the example replicates the
creation-ID and deal derivations from the fixture's private seed file. Before
every search it **checks itself** against the public factory: the v2 `b_` ID
and the actual board-started deal must match. If they differ, it fails with
"derivation drifted". No rack, seed or key reaches browser metadata.

## Observable contract

Numbers refer to the acceptance mapping below. "Desktop" means 1280×900 without
touch. "Mobile" means 390×844 with `hasTouch`.

1. **Catalog and selection.** The browser accepts a catalog whose tile profile
   contains `letters` faces. The tile profile and word list are selected
   independently: changing one selector leaves the other two unchanged.
2. **Creation tile grid.** The "Tile distribution" view of the selected profile
   shows each multi-letter face whole (`CH`, `SCH`), never as `□`, as a
   truncated `S`/`C`, or split into several tiles. Its value and count stay
   visible. If the faces are shown as separate tile elements (as in PR #118),
   each face fits its element on desktop and mobile (see 10). The creation page
   has no horizontal scroll at 390 px.
3. **Authoritative compatibility.** The `Authored two-letter pairs` list with
   the sample tiles is rejected by the server: an alert appears, no join link
   is created, `hostedMatches` is unchanged and the selections are kept.
4. **Real match.** Creating with the sample selection produces a real hosted
   match. Its public `configuration.tiles` equals the catalog's sample tiles,
   and the bag holds 29 − 2 × 7 tiles after the start.
5. **Private rack.** Each physical tile takes one rack entry; a rack still has
   7 entries. A multi-letter tile shows its whole face and its single
   configured value on one tile. Its accessible name keeps the existing rack
   pattern with the whole face, for example `CH, 5 points` and
   `SCH, 8 points`.
6. **Editable draft.** A multi-letter tile occupies one editor square. The
   square's name keeps the existing pattern with the whole face:
   `Row 8, column 8: tentative CH`, and after commit
   `Row 8, column 8: committed CH`.
7. **Shared-board preview and committed board.** One cell, one tile, the whole
   face and its value: `Row 8, column 8: Tentative CH, 5 points` and
   `Row 8, column 8: Committed CH, 5 points`. CHAT shows exactly three tiles.
   `public.board` holds `CH`, `A`, `T` at (7,7), (7,8), (7,9).
8. **Dictionary on the spelled word.** CH + A spells CHA. Committing it gives
   `Not in the dictionary: CHA.` with no canonical change: the board, history
   and both racks stay as they were, and the draft is kept. Adding T commits
   `words: ["CHAT"]` over three cells. Lin then places SCH and A above the
   committed CH, which commits `["SCHACH"]` through it.
9. **Single scored value.** `move_score` equals the sum of the configured face
   values, each counted once, with the classic premiums of newly covered
   squares. For CHAT that is (5 + 1 + 1) × 2 = 14 on the double-word centre.
   For SCHACH it is 8 + 1 + 5 = 14. After the game ends through scoreless
   passes, each remaining rack tile's value is deducted once.
10. **Legibility.** Every visible `CH`/`SCH` face lies completely inside its
    tile or square: no overflow, clipping or ellipsis. Its computed font size is
    at least **9 px**. A long face may be drawn smaller than a single letter.
    This holds for the desktop rack, desktop draft, mobile rack, mobile draft
    and committed square, the shared board (preview and committed) on desktop,
    and the shared board at 390×844. The mobile phone has no horizontal page
    scroll. Screenshots of all of these are attached to the Playwright report.
    No new committed baselines are required.
11. **Input.** Ada (desktop) places tiles and commits using only Tab, the arrow
    keys and Enter. Lin (mobile, `data-input="touch"`) places tiles and commits
    using only taps.
12. **Blank assignment.** The blank chooser offers exactly the profile's
    single-letter faces, in profile order (A, E, N, S, T, C, H), and never `CH`
    or `SCH`. Escape cancels it without a draft placement. Locally, a draft
    assigning `CH`/`SCH` (or a letter on no face) to a blank is rejected with
    the `blank` problem. The engine rejects it as well (#116).
13. **Turn history.** `Turn 1: Ada — CHAT · 14 points` and
    `Turn 2: Lin — SCHACH · 14 points`. Both stay unchanged after the board
    reloads in the finished match.
14. **Reconnect.** After a phone reload, the rack has the same tile IDs and the
    same whole faces (`SCH, 8 points`), and the committed `CH` square is still
    shown. After a board reload, the committed faces and history are restored.
15. **Accessibility.** axe reports no violations on the board and on both
    phones once multi-letter tiles are on the rack, preview and board.
16. **Unchanged behaviour.** Single-letter profiles keep their exact wire and
    canonical bytes (#116 native pins). All existing browser, unit and Rust
    suites, including the Pages `performance-budgets.spec.ts` 50 ms interaction
    contract and the existing screenshot baselines, pass unchanged. The browser
    rejects a 16-letter word in a single-letter ruleset as it does today
    (`game-protocol.test.ts`).
17. **Client and protocol negatives** (browser schema, `bun test`):
    - A `letters` face must have at least 2 characters, each alphabetic and not
      lowercase, and at most 4 UTF-8 bytes. It is rejected when it is empty,
      `C`, `ch`, `Ch`, `sCH`, `C1`, `C H`, `C-`, `SCHTZ` or `ÄÖÜ`, has extra or
      missing fields, or is `letter: "CH"`. Accepted examples: `CH`, `SCH`,
      `TSCH`, `ÄU`, `ÄÖ`, `DŽ`.
    - A ruleset may hold `C`, `H`, `S`, `CH` and `SCH` together, but not a
      duplicate face.
    - Board and preview `letter` values follow the same character rule with
      1–4 bytes. A tile with `is_blank: true` must have exactly one character.
    - A committed history word may exceed 15 characters, but never
      `board_size × longest configured face` characters (45 with SCH on 15×15).
      Single-letter rulesets therefore keep their limit of 15.
    - The public catalog schema accepts the sample profile.

Any accessible name, copy or layout not quoted above is an implementation
choice. Examples are how the value is placed inside a tile and how a long face
is scaled. Only the observable rules above are fixed.

## Acceptance mapping

| Item  | Test                                                                                                                                                              |
| ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1–3   | `multi-letter-tiles.spec.ts`: "the multi-letter sample is selected independently and its whole faces are listed"                                                  |
| 4–15  | `multi-letter-tiles.spec.ts`: "a real match plays CH, A, T as CHAT and SCH, A through CH as SCHACH on whole-face tiles"                                           |
| 5, 7  | `multi-letter-tiles.test.tsx`: "rack and shared board render one tile per face with its single value"                                                             |
| 6, 12 | `multi-letter-tiles.test.tsx`: "drafts place a multi-letter tile on one square and never assign CH or SCH to a blank" (a lone CH is `no-word`, as the engine has) |
| 17    | `multi-letter-tiles.test.tsx`: faces, rulesets/catalog, snapshots/history bound tests                                                                             |
| 16    | the unchanged existing suites; native pins in `crates/scrabble-game/tests/multi_letter_tiles.rs`                                                                  |

On `a60d72e`:

- the unit file fails 5 of 5, because the schema rejects the `letters`
  discriminator;
- without the gate, the browser spec fails 2 of 2 because the catalog lacks the
  sample;
- with `SCRABBLE_BROWSER_MULTI_LETTER=1`, it fails 2 of 2 because the browser
  cannot parse the catalog and no selectors appear.

A throwaway prototype was used only to check that this contract can be met; it
was never committed. Its first naive rendering failed item 10 (`SCH` overflowing
a board cell, and then 7.7 px on the mobile board). Once those were fixed, both
browser tests and all unit tests passed.

Implementation notes that follow from the contract:

- the test-only `apps/web/browser/populated-performance-state.ts` reads
  `face.letter` after excluding blanks, so it must be adapted to the widened
  face type;
- the journey adds one hosted match to the dev suite, which then finishes. If
  the suite reaches the fixture's match capacity, record the need to raise the
  capacity (see PR #119) rather than weakening this journey.

Commands (low-memory settings as used for this contract):

```sh
NODE_OPTIONS=--max-old-space-size=3072 bun test apps/web/src/multi-letter-tiles.test.tsx
NODE_OPTIONS=--max-old-space-size=3072 CARGO_BUILD_JOBS=2 \
  bun scripts/test-browser.ts multi-letter-tiles --workers=1
```

The completion gate stays `bun run check`.

## Public Pages verification (after deployment, not in CI)

Do this after a production-like Pages build has been checked against a reachable
server, and after the frontend and server have been deployed together:

1. Open <https://moritzbrantner.github.io/scrabble/>. Confirm `GET /profiles`
   on the public API lists the multi-letter tile profile and a compatible word
   list.
2. Select that tile profile and the word list independently, leave the board
   as it is, and confirm the tile distribution shows `CH` and `SCH` whole.
   Create the match.
3. Join from two independent browser contexts: a desktop browser and a phone or
   a 390×844 touch emulation. Start the match.
4. Play a word that uses a multi-letter tile from the dealt rack (for example
   CHAT when CH, A and T are dealt). Use a blank or exchange tiles if needed.
   Then observe:
   - the rack, draft, board preview, committed board and turn history all show
     the whole face on one tile with its value;
   - the spelled word appears in the history;
   - the score uses the face value once.
5. Try one word that is not in the list and confirm it is rejected with no
   board, score or rack change.
6. Reload one phone and the board, and confirm the faces and history survive.
7. Record the URL, the match's dictionary and ruleset identities, and the
   desktop and phone screenshots on the issue.

Fixture results do not count as public success.

## Open questions

1. **Public sample (resolved by Atlas 1.41: English CH/SCH).** The original
   question was: which multi-letter sample does the owner expect on the
   public site? The routine default above is the English tiles plus CH and SCH,
   with the existing English word list. The alternative is a German sample with
   a German word list (the issue's CH/SCH example is German). The repository has
   no public German word list, so that would mean choosing a new list and its
   licence. Atlas 1.41 chose the English sample without acquiring a German word list.
2. **Spellability** (inherited from multi-letter-tiles.md). The sample relies
   on composition checking face letters only, so a word list may contain words
   that no face segmentation can spell. The contract does not require rejecting
   such lists.
