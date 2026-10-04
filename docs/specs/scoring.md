# Scoring and premium consumption

`scoring::calculate` accepts only a structurally validated move and returns ordered word scores, one bingo bonus, and a total. It reads canonical tile values and the immutable ruleset; it never changes state. Letter premiums modify a new tile's contribution before all word multipliers are multiplied together. The main word and every cross word score independently; a new premium at an intersection applies to each affected word.

Committed tiles always contribute their unmodified canonical value. Board occupancy therefore consumes premiums without a duplicated spent-square store. Blanks always contribute zero, including on letter premiums, while a new blank on a word premium still multiplies the word. Playing exactly the configured rack size adds the configured bingo bonus once after summing every word. Clearing a partially depleted rack earns no bonus.

All score arithmetic is checked in u32. The cumulative player score is checked in i32 before any tile transfer. Overflow fails explicitly rather than wrapping or saturating.

`scoring::commit_placement` is the storage-level scored commit: it verifies active-player authority, derives structure and score, checks the updated total, transfers tiles, and sets the cumulative score only on success. It does not expose a caller-supplied score. Rejected or repeated placements leave both board and score unchanged. This seam intentionally does not advance turns, draw tiles, validate dictionary membership, or admit browser commands. `commit::apply` composes this storage seam with installed-dictionary validation and atomic turn advancement, and `GameSession::apply` admits browser commits. #24 adds deployment dictionary selection.

`cargo test -p scrabble-game --test scoring --locked` covers stacked letter/word premiums, each spent premium kind, letter and word intersections, new and committed blanks, configurable full-rack bonuses with multiple cross words, partially depleted racks, pure scoring, cumulative updates only at commit, repeated placement rejection, non-active players, and arithmetic overflow with unchanged canonical bytes.
