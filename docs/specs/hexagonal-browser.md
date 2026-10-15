# Hexagonal match: independent integrated acceptance for #96

Authority: current Atlas 1.3, 1.4, 1.10–1.13, 1.15, 1.21, 1.23, 1.31,
1.37 and 1.43; issue #96. Baseline `360f89b9546ed6991af9e81271bf838237c22c16`.
This separate acceptance context authors the continuation before production edits.
The existing native engine is a prerequisite, not delivery of this whole issue.

## Observable journey and authority

On the actual Pages app, independently choose a word list, tile profile and hex
board profile; join with two private player contexts; start, draft and commit a
center-crossing opening; take connected turns on all three axes; exchange/pass,
finish, and reload/reconnect. Exercise both premium policies, and combined CH/SCH
faces with both tile-count and spelled-letter main-word metrics. The large shared
board and compact phone controller show the actual hex layout and agree with the
same authoritative match. Production-like browser evidence precedes merge; public
acceptance after the exact deployment precedes completion of #96.

Before commit, both board and player can inspect the authoritative main word,
length metric, deterministic tie-break explanation, each formed word's score and
total. A preview cannot alter committed board, score or turn. Cancel clears only
the tentative draft/preview. Accepted results and their explanations remain
consistent after reconnect and canonical replay. This contract does not prescribe
an API endpoint, transport representation, UI wording or rendering technology.
It forbids implementing another rules/scoring authority in the browser.

## Obligation and evidence map

`hexagonal.rs` below means `crates/scrabble-game/tests/hexagonal.rs`.
`multi_letter_tiles.rs` means its sibling native test file. Native tests are
necessary correctness evidence, not substitutes for the integrated rows.

| Issue obligation                                                                    | Existing independent coverage                                                                                                                                                                                              | Integrated continuation required before completion                                                                                                                                                                                                                                                                |
| ----------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Immutable independent board identity, valid regular hex/radius/center/premiums      | `hex_geometry_is_validated_with_one_premium_per_on_board_cell`; existing server `profiles`/configuration-store contracts                                                                                                   | Catalog/creation selections retain dictionary and tiles when changing board; real preview and match each contain exactly the on-board hex cells; both sample premium policies selectable                                                                                                                          |
| Preserve square matches, rules, serialized bytes and existing profile identity      | `square_rulesets_states_and_snapshots_stay_byte_identical`, existing profile/recovery tests                                                                                                                                | Keep existing square BoardProfile identity/fingerprint inputs unchanged; old square creation clients remain usable despite expanded catalog capability; existing square browser suites and recovery remain green. Verify mechanism independently without prescribing negotiation versus compatible representation |
| Three straight axes; center; connectedness; no bends/gaps/off-board placement       | `words_extend_along_each_of_the_three_hex_axes`, `opening_word_must_cover_the_center_hexagon`, `off_board_hex_cells_are_rejected_without_mutation`, `bent_square_diagonal_gapped_and_disconnected_placements_are_rejected` | Actual keyboard and touch hit targets cover the same cells/axes; successful connected turns, rejected invalid draft/commit and cancel leave authority intact                                                                                                                                                      |
| Top-to-bottom down-left spelling; reverse spelling never substituted                | New `diagonal_reads_top_to_bottom_and_never_substitutes_the_reverse_word` under Atlas 1.43                                                                                                                                 | A visible non-palindromic diagonal word appears in the same top-to-bottom order in editor, shared preview, precommit explanation and accepted history                                                                                                                                                             |
| Every new main/cross word validated and scored; invalid crossing rejects atomically | `an_invalid_cross_word_on_the_third_axis_rejects_the_whole_move_without_mutation`                                                                                                                                          | Real server rejects illegal crossing with perceivable feedback; board, scores, turn, racks and dictionary remain unchanged; no other player's rack or bag data leaks                                                                                                                                              |
| Both premium policies; double/triple word/letter; consume only on accepted new tile | `every_new_word_scores_under_both_premium_policies_and_premiums_are_consumed`                                                                                                                                              | Precommit breakdown and actual accepted score agree under both policies; intersection yields different cross-word premium application; later extension does not reuse premium                                                                                                                                     |
| Single-tile longest main word; deterministic ties                                   | `a_single_tile_selects_the_longest_formed_word_as_main_word`, `equally_long_single_tile_words_use_the_fixed_axis_tie_break`                                                                                                | Show selected main word, metric and per-word/total consequences before commit on board and phone; equal-length tie stays identical after reconnect/replay                                                                                                                                                         |
| Combined CH/SCH hex, tile-count default versus spelled-letter metric                | `multi_letter_tiles.rs::hex_main_word_length_counts_cells_or_spelled_letters` pins AAA/ASCH and totals 15 versus 21; other #98 tests pin physical tile value, blanks, dictionaries and conservation                        | Actual multi-letter authoritative hex matches under both metrics, with potentially different main words and premium totals visible on board and phone. Reuse existing #98 ownership; necessary deployment/source-sample work in existing #121 is a dependency/handoff, not silently invented fallback content     |
| Blank assignment, exchange/pass, turn progression and final scoring                 | Existing `multi_letter_tiles`, `exchange`, `turns`, `completion` native suites                                                                                                                                             | Actual hex controls assign a single-letter blank, exchange/pass advance correctly, and a finished match agrees on board and players; keep malformed/dictionary rejection boundaries precise                                                                                                                       |
| Preview, retry, privacy, reconnect/recovery and replay                              | `hex_match_serializes_reconstructs_and_replays_canonically`; existing protocol/session/replay and private-rack suites                                                                                                      | True hex precommit preview/breakdown, no committed mutation before acceptance, cancellation, reconnect preserving board/rack/identity, no duplicate commands/score, same history/main-word decisions after replay. Recovery must reconstruct hex metadata rather than silently square defaults                    |
| Complete geometry-dependent wire/browser bounds                                     | Existing square/face-length schema and rejection contracts                                                                                                                                                                 | A legal 15-tile hex move may form 31 words (one main plus two crosses per tile); valid hex history/rejection must not be rejected by old square-only 16-word limits. Preserve malformed-input bounds and single/multi-letter word-length constraints                                                              |
| Actual board/controller roles, phone/desktop access, performance                    | Existing settings, mobile, controlled 50 ms interaction and multi-letter legibility suites                                                                                                                                 | True hex preview/editor/shared-board screenshots; actual touch and keyboard selection, unobscured cells/rack/actions, accessible labels/focus and axe; portrait/landscape. Existing interaction-performance contract remains unchanged                                                                            |
| Publication and actual owner outcome                                                | No public hex acceptance exists on baseline                                                                                                                                                                                | Real production-like build/server exercise before merge, then exact deployed public route and live two-player journey including both policies; explicitly report physical-device and restart evidence limitations                                                                                                 |

## Test and fixture ownership

Reuse existing framework, native API and real server fixture. Deterministic deals
may select real creation inputs through the fixture's native factory, as #98 does;
never grant racks, bypass authoritative admission, patch board state from the
browser or replace server projections with synthetic ones. Authored representative
word lists are acceptance inputs; passing them does not establish production
word-list breadth. Expected named words/scores must come from the independent
scenario, not be copied from the implementation output.

A new catalog/geometry precondition failing on baseline records only that missing
boundary. It does not prove later scripted moves or harness mechanics ran. Any
fixture/helper change affecting exercised state requires separate re-verification.
No fully implemented row may be inferred from this obligation map: it records
required coverage, including explicit pending integrated work.

Run the complete source-owned `bun run check` on the final candidate because
inspection metadata does not establish a complete affected-test graph. Use focused
cases during authoring. Keep raw baseline failures, later runs and publication
checks separate and retain their source identities.

## Initial pre-implementation evidence

Native command: `python3 ~/.local/share/coding-agent/disk-maintenance.py run -- cargo test -p scrabble-game --test hexagonal --locked`.
Against the pinned baseline plus independent tests, **12 passed, 0 failed/ignored**,
including the new non-palindromic owner-direction case. Complete output is
`/tmp/scrabble96-native-baseline.log`. Its disposable target was cleaned after exit.

Browser command: `bun scripts/test-browser.ts hexagonal-board.spec.ts` with the
repository-declared Bun 1.4.2 first on PATH. **2 executed, 2 failed** on desktop
1280 px and phone 390 px: the actual real-fixture-backed board selector has zero
hex options, rather than at least the two policy samples. The configured native
server and actual browser creation controls were reached. Later preview geometry,
independent-selection and screenshot assertions remain unexecuted on baseline.
Complete output: `/tmp/scrabble96-browser-catalog-baseline.log`; first-failure
screenshots and traces remain under `test-results/hexagonal-board-*`.

These first cases establish an actionable missing integration boundary. They do
not yet author or demonstrate every integrated obligation above. The draft remains
an acceptance work in progress; production implementation must wait for the complete
independent handoff and any necessary fixture/combined-profile dependency decisions.
