# Public English CH/SCH preset (#121)

Independent pre-implementation acceptance on baseline
`783e2d05061381f6b8f75fef4a7ca7dc4bc64bac` (merged #122), following current
Atlas Decisions 1.21, 1.25, 1.31, 1.41 and priority 4.1. Decision 1.41 resolves
the historical public-sample question in [multi-letter-browser.md](multi-letter-browser.md).
This contract adds production-catalog evidence; it reuses the delivered browser
and engine acceptance rather than defining another game implementation.

## Scope and obligation map

| Obligation                                                                                                                | Required independent evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Additional opt-in English CH/SCH profile from the actual unconfigured-catalog production path, with real `scowl-en-us-60` | `server_process.rs::builtin_multiletter_preset_real_scowl_cli_catalog_and_creation`: real executable, no operator catalog, exact 78,026-word SCOWL content hash; exactly one additional multi-letter profile with CH/SCH and all ordinary English faces/blank, nonempty label, distinct identity. No prescribed counts, values, identity name or label text.                                                                                                                                                                                                 |
| Server-authoritative compatible composition, independent tile/word/board choices                                          | The same test selects the actual preset with every actual fallback board and unchanged SCOWL selection; real v3 HTTP creation and native snapshot retain full tile definitions and dictionary. Existing `profiles.rs::independent_sources_compose_to_one_unchanged_engine_configuration` and `server_process.rs::profiles_catalog_rejections_retry_conflicts_and_legacy_defaults_do_not_allocate_unwanted_games` cover independent alternatives and HTTP compatibility rejection without unwanted allocation.                                                |
| Deny incompatible sources; immutable identity and recovery                                                                | New CLI case feeds the actual published preset definitions to `compose_profiles` with unsupported Unicode words and requires `UnsupportedLetters`; stale actual tile identity is rejected by the real HTTP API without allocation. Existing `profiles.rs` content/source identity cases, `multi_letter_profiles.rs` and configured restart/replay case in `server_process.rs` preserve immutability. The public preset has only one bundled word list; the Unicode input is an explicit negative test, never a production source or claimed public pair.     |
| Classic untouched; legacy v2, single-letter rules and defaults remain compatible                                          | New ordinary `server_process.rs::builtin_multiletter_preset_preserves_untouched_classic_v2_creation` checks fallback default distribution and `legacyCompatible: true`, then actual v2 creation/snapshot against the full unchanged `english_fixture()`. Existing `multi_letter_tiles.rs::single_letter_rulesets_states_and_snapshots_stay_byte_identical` pins canonical bytes; existing square/browser suites remain required.                                                                                                                             |
| Actual catalog parser compatibility and independent selectors; whole-face creation distribution                           | Parse the new CLI case's optional catalog output with the production browser `profileCatalog` schema, then run existing `multi-letter-tiles.test.tsx` and browser `multi-letter-tiles.spec.ts`. Actual public frontend must expose this actual fallback preset and preserve other selectors while changing tiles. Fixture UI proves the browser seam, not public availability. `/profiles/hex` is the modern catalog surface introduced by #122; retain its existing strict legacy `/profiles` projection behavior rather than requiring hex metadata there. |
| One physical rack tile, one board cell, full spelling and one configured value; valid/invalid spelling and final score    | Reuse `crates/scrabble-game/tests/multi_letter_tiles.rs` and `crates/scrabble-server/tests/multi_letter_profiles.rs`, `apps/web/src/multi-letter-tiles.test.tsx`, and the real-server browser journey in `apps/web/browser/multi-letter-tiles.spec.ts`. Its authored dictionary/deal are fixture evidence. Actual fallback random deals, public SCOWL spelling and preset values still require live play. Never copy fixture numeric scores into production expectations.                                                                                    |
| Desktop/phone input, whole faces at ≥9 px, accessibility, rejection without mutation, reconnect/history/screenshots       | Reuse the complete #98 browser journey and its existing assertions, then repeat actual public actions below with independently admitted clients. Phone emulation does not establish physical-device acceptance. The existing 50 ms browser performance contract remains unchanged.                                                                                                                                                                                                                                                                           |
| Client-first publication before server enables CH/SCH                                                                     | Operational evidence: exact successfully deployed Pages revision/artifact compatibility must be recorded before exposing the new server preset. Then record native server source and binary identity. A merged PR or successful local parser alone does not establish publication.                                                                                                                                                                                                                                                                           |
| Actual hosted gameplay and public finish                                                                                  | Required live journey below, real match identifiers, dictionary/rules identities, revisions, independent screenshots and reconnect results. This remains a release gate; local native/browser tests cannot satisfy it.                                                                                                                                                                                                                                                                                                                                       |
| #96 dependency: four actual public hex modes, combined faces and meaningful crossing                                      | Follow [hexagonal-browser.md](hexagonal-browser.md) and #96's existing handoff. All-word/main-only × tiles/letters require real CH/SCH gameplay, a crossing whose premium/metric consequences differ, and precommit/accepted/reconnect explanations. A preset composing with four boards proves only admission, not those four public journeys.                                                                                                                                                                                                              |

The new external-content native case is explicitly ignored in the ordinary suite
because SCOWL is prepared release content, not a tracked test fixture. **It is a
required explicitly invoked #121 gate**, not an optional successful skip. Missing
content, wrong content/hash or zero executed cases is missing acceptance. Keep
this extra command alongside the complete source-owned `bun run check` before
merge; the test-impact graph is incomplete, so focused tests cannot replace that
gate. Existing tests named above must be rerun on the final head, not inferred
green from older merged work.

## Focused commands

Prepare content by the existing `scripts/prepare-dictionary.ts` recipe if the
local release content is absent; do not label an authored word subset SCOWL.

```sh
SCRABBLE_ACCEPTANCE_SCOWL_FILE="$PWD/.local/deployment/content/words.txt" \
SCRABBLE_ACCEPTANCE_CATALOG_OUTPUT=/tmp/scrabble121-actual-catalog.json \
python3 ~/.local/share/coding-agent/disk-maintenance.py run -- \
  cargo test -p scrabble-server --test server_process builtin_multiletter_preset \
  --locked -- --include-ignored

bun -e 'import { profileCatalog } from "./apps/web/src/profile-model"; const catalog = profileCatalog.parse(await Bun.file("/tmp/scrabble121-actual-catalog.json").json()); if (!catalog.tileProfiles.some(row => row.tiles.some(tile => tile.face.kind === "letters"))) throw Error("actual fallback has no multi-letter preset");'

bun test apps/web/src/multi-letter-tiles.test.tsx
python3 ~/.local/share/coding-agent/disk-maintenance.py run -- \
  cargo test -p scrabble-server --test multi_letter_profiles --test profiles --locked
python3 ~/.local/share/coding-agent/disk-maintenance.py run -- \
  cargo test -p scrabble-game --test multi_letter_tiles --locked
bun scripts/test-browser.ts multi-letter-tiles.spec.ts --workers=1
bun scripts/test-browser.ts --pages multi-letter-tiles.spec.ts --workers=1
```

Use the repository-declared Bun version. Catalog output is optional test evidence,
never a runtime configuration. Keep failed and subsequent logs separately.

## Public release journey and evidence gaps

1. Before native deployment, independently verify that the public
   <https://moritzbrantner.github.io/scrabble/> assets match the successful
   CH/SCH-capable frontend revision. Record workflow/artifact/deployed asset
   identity and actual browser compatibility. Preserve the client-before-server
   chronology; the older client cannot parse `letters` faces.
2. Publish the authoritative server only after that evidence; record source
   revision, binary identity and actual modern public catalog response. Confirm
   untouched defaults remain classic and legacy compatible. Select the actual
   English sample independently of SCOWL and board and inspect whole-face counts
   and values in the creation grid; no automatic substitution.
3. Admit two real independent browser contexts through public UI. Record match,
   selected source identities and effective rules, with desktop keyboard and
   phone/touch input. Play actual dealt CH and SCH tiles in valid SCOWL words;
   CHAT on three cells is an example, not a guaranteed deal. Use real legal
   exchange/play to obtain useful racks. Do not inject private state or choose
   product counts/values to reproduce an authored test trace.
4. Independently calculate score from selected physical tile values and newly
   covered premiums. Capture whole faces in rack, draft, shared preview, committed
   board and history; verify each face occupies one slot/cell. Reject an invalid
   spelled word and confirm board, score, turn and racks do not change.
5. Reload phone and shared board; verify identities, whole faces, board/history
   and unchanged scores. Finish through legal play/pass, account for actual
   remaining physical-tile values, and capture desktop and phone screenshots.
   Record exact public URLs and client/server revisions. State emulation, restart
   or physical-device limitations rather than inferring evidence.
6. Keep #96 open until all four actual public hex policy/metric journeys and
   differing crossing behavior are independently evidenced. Native SCOWL planning
   with a rich test distribution/direct seed and fixture browser traces do not
   establish production factory deals or public acceptance. Existing #96 trace
   planning may help, but lack of its random deal is an evidence gap, not grounds
   to tune product defaults or weaken acceptance.

At the initial handoff, public preset availability, hosted multi-letter gameplay,
actual fallback CH/SCH parser acceptance, production-like browser continuation and
all four #96 public hex journeys are **unrun/pending**, not passed. The parent
context is establishing client-publication evidence separately; this acceptance
context has not independently claimed that operational result.

## Initial baseline evidence

Against the pinned baseline plus these independently authored tests:

- `/tmp/scrabble121-cli-baseline.log`: actual executable **1 passed, 1 failed,
  0 ignored, 15 filtered**. Classic/v2 invariant passes. Real pinned SCOWL is
  read, hashed and loaded into the actual no-operator-catalog executable, and
  `GET /profiles/hex` returns successfully with legacy-compatible defaults. The
  first new product precondition fails: **0 multi-letter profiles versus 1**.
  Later sample composition, stale identity, all-board creation and gameplay were
  not reached; no claim is made about them.
- `/tmp/scrabble121-client-baseline-declared.log`: declared Bun 1.4.2, existing
  client schema/render/draft acceptance **5 passed, 0 failed, 56 assertions**.
  Earlier system Bun 1.4.3 output is separately preserved at
  `/tmp/scrabble121-client-baseline.log`.
- `/tmp/scrabble121-cli-baseline-final.log`: final acceptance-source rerun gives
  the same **1 passed, 1 failed, 0 ignored, 15 filtered** result.
  `/tmp/scrabble121-actual-parser-baseline.log`: declared Bun 1.4.2 parses the
  actual baseline CLI catalog successfully, then fails the explicit required
  multi-letter availability assertion. This is absence evidence, not a parser
  failure or proof the new catalog is already accepted.

The native command uses a disposable Cargo target cleaned by the disk maintenance
wrapper. Raw final authoring-run output is kept separately in
`/tmp/scrabble121-cli-baseline-final.log`; its catalog output supports the actual
parser command above. Full completion and browser gates were deliberately not
run during this acceptance handoff.

Shared convention resolution `sourceRevision`:
`5ad0db36974c8b0e5f9812c90ee34a4f227852c3`.
