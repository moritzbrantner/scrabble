# Authoritative hexagonal board rules

This headless enabler of #96 (issue #110) lets the authoritative `scrabble-game`
engine run a complete match on a hexagonal board-layout profile. It implements
Atlas Decisions 1.4, 1.10, 1.11, 1.12, 1.13, 1.15, 1.23 and 1.31 and extends the
single authoritative board boundary required by
[match-profile composition](match-profile-composition.md). React never emulates
these rules. The catalog of sample hex profiles, protocol/browser rendering, hex
hit targets, multi-letter faces (#98) and the board editor (#101) are out of
scope.

## Board geometry in the effective `Ruleset`

`Ruleset` gains three immutable board-profile fields. Each has a default that
equals today's behavior. Each is omitted from JSON while it holds that default
(`#[serde(default, skip_serializing_if = ...)]`):

| Field              | Type                        | Values (JSON)                | Default    |
| ------------------ | --------------------------- | ---------------------------- | ---------- |
| `board_shape`      | `ruleset::BoardShape`       | `square`, `hexagon`          | `square`   |
| `premium_policy`   | `ruleset::PremiumPolicy`    | `all_word`, `main_word_only` | `all_word` |
| `main_word_length` | `ruleset::WordLengthMetric` | `tiles`, `letters`           | `tiles`    |

All three enums are unit enums. They derive `Clone, Copy, Debug, Default, Eq,
PartialEq, Serialize, Deserialize` and use `rename_all = "snake_case"`.
`Ruleset` keeps `deny_unknown_fields`, so an unknown field or value is still
rejected.

**Square boards are unchanged.** A square ruleset keeps `board_size` as the odd
side length (1–15) and row-major `premiums` with one premium per cell. Its
serialized JSON, canonical state bytes and public snapshots are byte-identical
to the pre-#110 engine. Decision 1.13 makes premium application a property of
hexagonal variants, and classic square scoring stays unchanged. Decision 1.31
says the server rejects invalid combinations instead of silently ignoring a
setting. Validation therefore rejects a square ruleset whose `premium_policy`
or `main_word_length` is not the default.

**Hexagon of radius R.** For `board_shape: hexagon`, the existing
`Coordinate { row, column }` storage holds the axial coordinates
`(q, r) = (column − R, row − R)` on a `(2R + 1) × (2R + 1)` grid:

- `board_size = 2R + 1` is the storage-grid side. It must be odd. The radius is
  `R = board_size / 2`, and it must satisfy `1 ≤ R ≤ 7`. The upper bound is the
  existing 15-cell `Coordinate` and board bound. No new, wider bound is
  introduced (Decision 1.37).
- A cell `(row, column)` is on the board iff `row ≤ 2R`, `column ≤ 2R` and
  `R ≤ row + column ≤ 3R`. The board therefore has `3R² + 3R + 1` cells.
- The unique center is `(R, R)`, the same `board_size / 2` formula as on the
  square board.
- `premiums` has exactly one entry per on-board cell, in row-major order of the
  storage grid with off-board cells skipped. Any other length is invalid.
- A placement on an off-board grid cell, or outside the grid, fails storage
  validation with `StateError::OutsideBoard`. Through `placement::validate`
  this surfaces as `PlacementError::Storage(StateError::OutsideBoard)`. A failed
  placement never mutates state.

`Ruleset::validate()` and therefore `GameState::new` reject malformed geometry
(even `board_size`, `R = 0`, `R > 7`) and wrong premium counts. They return
`ContractError` and `StateError::InvalidRuleset` respectively.

### The three axes

`placement::Direction` gains a third variant. Each axis has a fixed unit step on
the storage grid and a fixed tile order inside a `Word`:

| `Direction`      | Step `(Δrow, Δcolumn)` | Axial step | Square board                           |
| ---------------- | ---------------------- | ---------- | -------------------------------------- |
| `Horizontal`     | `(0, +1)`              | `(+1, 0)`  | unchanged                              |
| `Vertical`       | `(+1, 0)`              | `(0, +1)`  | unchanged                              |
| `Diagonal` (new) | `(+1, −1)`             | `(−1, +1)` | never used; `(+1, −1)` stays `NotLine` |

The six hexagonal neighbours of a cell are the cells one step away along these
axes in either sense. A square-diagonal step `(+1, +1)` is not a hex axis and
does not make cells adjacent. `Horizontal` and `Vertical` words keep the square
reading order: increasing column, then increasing row. On a pointy-top
rendering, the `Vertical` axis runs down-right and keeps both the
left-to-right and top-to-bottom conventions. The `Diagonal` axis runs down-left.
Its tiles are ordered by increasing row (top to bottom), as approved by Atlas
1.43 (Answers 1.42). Down-left words therefore read from top-right toward
bottom-left. The reverse spelling is not accepted merely because it appears in
the dictionary. Independent non-palindromic acceptance pins this direction.

## Hex placement validation

`placement::validate` keeps its signature and `PlacementError` variants. On a
hexagon:

1. Storage checks run first (ownership, duplicates, occupancy, on-board cells,
   blank assignment).
2. All new tiles lie on one of the three axes (`NotLine` otherwise). A bent
   line or a square diagonal is `NotLine`.
3. The new tiles and any committed tiles between them form one contiguous run.
   An empty cell between new tiles is a `Gap`. Committed tiles may fill gaps and
   extend either end.
4. The opening move covers the center `(R, R)` (`CenterRequired`, Decision
   1.12). A single tile forms no word (`NoWord`).
5. A later move must join committed tiles through at least one formed word
   (`Disconnected`). Adjacency is hexagonal adjacency.

Words are maximal contiguous runs of at least two tiles. **Main word** for a
move of two or more tiles: the run along the tiles' line. **Main word** for a
single tile: among the words formed through it on the three axes, the longest
by `main_word_length` (Decision 1.15). `tiles` counts occupied cells. `letters`
counts the letters of the spelled word. The two are equal for single-letter
faces, and multi-letter faces come with #98. **Tie-break:** the fixed axis order
`Horizontal`, then `Vertical`, then `Diagonal`. This order is independent of
input order, client and replay. The existing square single-tile rule does not
change.

`ValidatedMove::words()` lists the main word first. Cross words follow: new
tiles in `(row, column)` order, and for each tile the non-main axes in the order
`Horizontal`, `Vertical`, `Diagonal`. Each formed word appears once. The same
order feeds dictionary validation, `MoveScore`, and the `TurnAction::Commit`
`words` history.

**Every newly formed word along every axis** is validated against the
configured word list (Decision 1.11). One invalid word rejects the whole move
through `CommitError::Dictionary(DictionaryError::InvalidWords { .. })`. Board,
racks, bag, scores, revision and history then stay byte-identical, just as for
other rejected commits.

## Hex scoring and the breakdown

Every newly formed word scores. Letter premiums apply before word multipliers,
just as on the square board. The premium policy is part of the immutable
ruleset (Decision 1.13):

- `all_word`: a premium under a newly placed tile applies to **every** new word
  that contains that tile.
- `main_word_only`: such a premium applies **only to the main word**. Cross
  words still score at face value.

A premium is consumed by occupancy. Committed tiles always contribute their
plain value, so a premium is never applied again on a later turn. The bingo
bonus is unchanged.

`scoring::MoveScore` keeps `word_scores()`, `bingo_bonus()` and `total()`, and
adds:

- `words() -> &[scoring::WordScore]`, in the same order and length as
  `word_scores()`. `WordScore` exposes `text() -> &str`,
  `direction() -> Direction` and `score() -> u32`.
- `main_word() -> &WordScore`, which is always `words()[0]`.

This lets #96 preview the authoritative main word and per-word scores before
confirmation (Decision 1.15). Square moves populate the same breakdown, and
their values are unchanged.

## Snapshots, canonical state and replay

`PublicSnapshot.configuration` and `GameState::canonical_bytes()` already carry
the complete effective `Ruleset`. For a hex match, that ruleset includes
`board_shape`, `board_size` (radius) and the per-cell `premiums`, which is
enough for a client to render the board. It also includes `premium_policy` and
`main_word_length` whenever they differ from the default. The canonical
evidence version stays `3`, because square bytes must not change. A ruleset
deserialized from canonical or snapshot JSON equals the original. Replaying the
same seed and the same accepted commands yields byte-identical canonical state.

## Acceptance

`cargo test -p scrabble-game --test hexagonal --locked` was authored
independently before implementation. It uses only the public engine API and
covers:

- geometry validation (radius, parity, premium count, hex-only settings on
  square, off-board placement);
- center opening rejected and accepted;
- words along each of the three axes;
- bent, square-diagonal, gapped and disconnected placements rejected;
- an invalid cross word on the third axis rejecting the whole move without
  mutation;
- multi-word scoring under both policies with word and letter premiums on
  intersection tiles, plus premium consumption on a later turn;
- single-tile main-word selection by length, and the axis tie-break;
- canonical serialization, reconstruction and session replay of a hex match;
- byte-identical square serialization, canonical state and snapshots, pinned
  against values captured on the pre-#110 baseline.

The existing `scrabble-game` suites stay green. The full gate is
`bun run check`.

## Resolved owner direction and integrated continuation

Atlas 1.43 resolves historical Question 1.42: diagonal words read top to bottom.
`diagonal_reads_top_to_bottom_and_never_substitutes_the_reverse_word` pins the
non-palindromic spelling under both premium policies and either input order.

The independent #96 integration contract is [hexagonal-browser.md](hexagonal-browser.md).
The completed headless engine does not establish catalog, browser or public acceptance.
