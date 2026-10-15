# Multi-letter tile faces

This headless enabler of #98 (issue #116) lets the authoritative `scrabble-game`
engine use **multi-letter tile faces** such as CH or SCH. It implements Atlas
Decisions 1.2 and 1.21 (Answers 1.7), the main-word length metric of Decision
1.23, and the word-list/tile-profile compatibility check of Decision 1.25. The
server catalog sample profile, the browser rack, draft and board rendering
(including the browser `letter` schema, which accepts one scalar today), the
Pages journey and deployment stay in #98.

## The face variant

`TileFace` gains one variant. Single-letter faces and blanks keep their exact
form:

```rust
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TileFace {
    Letter { letter: char },
    Letters { letters: String },   // new, #116
    Blank {},
}
```

A multi-letter face serializes as `{"kind":"letters","letters":"CH"}`. Unknown
fields and kinds are still rejected. A ruleset without multi-letter faces never
mentions the variant. Its JSON, canonical state bytes and public and player
snapshots are therefore byte-identical to the pre-#116 engine, the same pattern
as the default-omitted fields in [hexagonal-board.md](hexagonal-board.md) and
[match-clock.md](match-clock.md). The canonical evidence version stays **3**.

`TileFace` can no longer be `Copy`. `Tile::face()` still returns a `TileFace` by
value, so `tile.face() == TileFace::Letter { .. }` comparisons keep working.

## Validation (`validate_tiles`, `Ruleset::validate`)

A `Letters` face is valid only if all of these hold:

- it has **at least two** letters. A one-letter `Letters` face would collide
  with the `Letter` face for the same letter, so it is rejected;
- every character is alphabetic and not lowercase. This is the rule that
  `WordList` uses for words, so `CH`, `SCH`, `ÄU` are valid and `Ch`, `ch`,
  `C1`, `C H`, `C-` are invalid;
- its UTF-8 encoding is **at most 4 bytes**. That is the size of the largest
  single-letter face today, so every per-cell string in board tiles, previews,
  history words and canonical evidence stays inside the existing worst-case
  budget. The native 65,535-byte snapshot limit is described in
  [configurable-rulesets.md](configurable-rulesets.md#validation-and-bounds).
  `CH`, `SCH`, `TSCH`, `ÄU`, `ÄÖ`, `DŽ` are valid. `SCHTZ` and `ÄÖÜ` are
  invalid;
- no two definitions share the same face string. Duplicate faces are rejected,
  just as duplicate single letters are.

A ruleset may hold single letters and multi-letter faces that share letters,
for example `C`, `H`, `S`, `CH` and `SCH`. Atlas 1.21 places multi-letter tiles
"alongside ordinary single-letter tiles", and the owner's German SCH example
needs S, C and H tiles. The existing bounds are unchanged: at most 64
definitions, at most 200 tiles, non-zero counts, one blank with value 0. An
invalid face is rejected by `validate_tiles`, `Ruleset::validate` and
`GameState::new` (`StateError::InvalidRuleset`).

## Gameplay

Each tile is **one physical tile** (Decision 1.21):

- **Placement:** a multi-letter tile occupies one cell. Line, gap, center,
  connection and `TooManyTiles` rules count cells and tiles, never letters.
- **Word text:** `Word::text()`, `WordScore::text()`, `TurnAction::Commit.words`
  and dictionary lookup use the **fully spelled** word. Each tile contributes
  its whole face, in board order. Placing CH, A, T in a row spells `CHAT` over
  three cells, and `Word::tiles().len()` is 3. The same holds for cross words
  through a new or committed multi-letter tile.
- **Dictionary:** a move is accepted only if every spelled word is in the word
  list. `InvalidWords` reports the spelled words (for example `["CHAT"]`).
- **Scoring:** the configured tile value counts **once** per tile, never once
  per letter. Letter premiums multiply that value, and word premiums apply as
  usual. Final rack deductions also count each tile value once.
- **Rack, bag, refill, exchange, bingo:** these count physical tiles. A rack
  holds `rack_size` tiles. A refill draws tiles up to that number, and
  `remaining_tiles` and `rack_count` count tiles. The `exchange_minimum_bag`
  check counts bag tiles. The bingo bonus applies when `rack_size` **tiles** are
  placed, however many letters they spell.
- **Blanks** keep the current single-letter semantics. `Placement.blank_as`
  stays one `char`, so a command that assigns `"CH"` to a blank fails to decode.
  A blank may become only a letter that has its own single-letter `Letter` face.
  If a ruleset has CH but no C, a blank cannot be C
  (`StateError::InvalidBlankAssignment`). A blank can never become a
  multi-letter face, which Atlas does not approve.

## Hexagonal main-word length (Decision 1.23)

For a single tile on a hexagonal board, `main_word_length: tiles` (the default)
compares formed words by the number of cells. `letters` compares them by the
length of the spelled word in characters. An SCH tile counts as 1 under `tiles`
and 3 under `letters`. The fixed axis tie-break is unchanged. Example: a new A
forms Horizontal `AAA` (3 cells, 3 letters) and Vertical `A`+`SCH` (2 cells, 4
letters). The main word is `AAA` under `tiles` and `ASCH` under `letters`. The
main-word-only premium policy, the breakdown order, the committed result and
the history all follow that selection.

## Wire and canonical projections

- `RackTile.face` (private rack) and `Ruleset.tiles` carry the `letters` variant.
- `BoardTile.letter` and `PreviewTile.letter` carry the tile's **whole spelled
  face** as a string (`"letter":"CH"`). For single letters and blank
  assignments, the bytes are unchanged, because a JSON `char` is already a
  one-character string. In Rust, these fields become `String`.
- Canonical evidence stores each committed tile's spelled face the same way.
  The full ruleset, which already sits in the evidence, carries the variant.
  Reconstructing the ruleset from canonical evidence and replaying the
  accepted commands produces identical canonical bytes and public snapshot.
- In-process accessors that return one `char` (`CommittedTile::letter`,
  `WordTile::letter`) must not silently truncate a multi-letter face. The
  exact replacement accessor is a routine implementation choice, and the
  tests only use the snapshot DTOs, `Word::text()` and `Tile::face()`.

## Server composition (Decision 1.25)

`TileProfile` validates faces through `validate_tiles`. It keeps multi-letter
definitions verbatim in the composed `Ruleset`, so faces are never dropped or
truncated. The word-list/tile-profile compatibility check in
`compose_profiles` and the deployment loader (`scrabble_server::dictionary::load`)
treat **every letter of every non-blank face** as supported. With only CH, A
and T tiles, a word list with `CHAT` is compatible. A word list with a letter
that appears on no face is rejected with `UnsupportedLetters`, which names the
missing letters. Whether a word must also be *spellable* by a face
segmentation is an open question (see below). This contract does not decide it.

## Acceptance mapping (#116)

Native acceptance lives in
`crates/scrabble-game/tests/multi_letter_tiles.rs` (public engine API only) and
`crates/scrabble-server/tests/multi_letter_profiles.rs`:

| #116 acceptance item | Test |
| --- | --- |
| CH + A + T spells CHAT across three cells | `ch_a_t_spells_chat_across_three_cells` |
| a cross word through a multi-letter tile | `cross_words_through_a_multi_letter_tile_score_its_value_once_with_premiums` |
| dictionary acceptance and rejection on the spelled word | `dictionary_validates_the_fully_spelled_words` |
| the tile scored once, with premiums | the two tests above, plus `final_rack_deductions_count_each_tile_value_once` |
| rack counts and bingo based on physical tiles | `racks_refills_and_bingo_count_physical_tiles`, `exchange_counts_physical_bag_tiles` |
| invalid face configurations rejected | `invalid_multi_letter_face_configurations_are_rejected` |
| blanks stay single-letter | `blanks_keep_single_letter_assignments` |
| hex `letters` vs `tiles` main-word selection | `hex_main_word_length_counts_cells_or_spelled_letters` |
| canonical serialization and replay | `multi_letter_matches_serialize_reconstruct_and_replay_canonically` |
| single-letter byte-identity pins | `single_letter_rulesets_states_and_snapshots_stay_byte_identical` |
| profile composition / compatibility | server: `tile_profiles_keep_multi_letter_faces_and_check_word_lists_by_face_letters`, `deployment_loader_accepts_words_spelled_with_multi_letter_faces` |
| bounds within transport/replay limits | server: `largest_multi_letter_public_snapshot_and_checkpoint_stay_inside_native_limit` |

The full `bun run check` remains the completion gate.

## Open questions (not decided here)

1. **Spellability.** Should composition reject a word list whose words cannot
   be segmented into the profile's faces? Example: `HAT` with only CH, A and T
   tiles. With a blank in the profile, a blank cannot become H either. The
   contract only requires that no face is dropped and that letters on no face
   are rejected.
2. **Only multi-letter faces.** May a profile have no single-letter faces at
   all? Today a ruleset needs at least one single letter. This contract does not
   decide whether that changes, and the tests always include a single letter.
3. **Blank assignment to a letter that exists only inside a multi-letter face**
   (H when only CH exists). This contract keeps the current rule (rejected),
   because Atlas leaves blank semantics unchanged.
