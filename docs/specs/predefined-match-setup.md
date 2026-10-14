# Predefined independent match setup

The next bounded #94 outcome implements Atlas 1.25, 1.31 and 1.39: independently
select available predefined word-list, tile and square-board profiles and exact
advanced-rule values before creating a real authoritative match. Current owner
priority 4.1 follows the #96 hex journey through this prerequisite. Baseline:
`72cf2cd19216533ee3abf6244875af81398f72ee`, including composition prerequisite #107.

## Catalog and creation boundary

Operators may configure `SCRABBLE_PROFILE_CATALOG_FILE`, an identified catalog
of predefined word lists (explicit name/revision/path), tile profiles and board
profiles. Source constructors and `compose_profiles` validate these inputs. The
existing deployment-selected configuration remains the v1/v2 creation default;
unset catalog configuration preserves that path rather than enabling authored
test dictionaries in production. The available catalog also supplies the
creation form's explicit existing default rules and selection.

`GET /profiles` returns version 1, public `wordLists`, `tileProfiles` and
`boardProfiles` metadata with immutable content identities and safe public tile
and square-board presentation, plus `defaults.selection` and `defaults.rules`.
It never returns word-list contents, initialization keys, capabilities or
private state. Catalog discovery uses the existing configured API origin.

`POST /games` gains an exact version-3 request containing `requestId`,
`requestedAt`, `selection` (`wordList`, `tile`, `board` content identities), and
`rules` (the six existing MatchRules fields, in their snake_case domain names).
Earlier version-1/2 requests and JoinInformation remain compatible. Missing,
unknown, stale, invalid and incompatible selections/settings reject without
allocating a match, substituting a source or changing another match. Structured
error reasons support translated, actionable UI feedback. Retrying identical
inputs with the same creation key recovers the same game; changing selected
sources or effective settings under that key returns an explicit conflict.
The separate shared-board proof remains private and verified by the server.

## Immutable initialization and recovery

An explicit `SCRABBLE_MATCH_CONFIGURATION_DIR` stores immutable initialization
manifests for configured matches and content-addressed word-list versions.
This state is separate from the upstream runtime's game recovery ownership.
Manifests contain public effective rules, selected source identities and a
word-list content reference, never private creation nonces or game state.
Prepare/fingerprint each dictionary once and share one immutable version across
matches; do not copy a complete word set for every new match.

Creation must durably bind initialization before a match can become available.
A restart or replay reconstructs the original effective configuration and exact
dictionary even after operator source edits; invalid/missing persisted input
fails closed rather than selecting current defaults. Existing v1/v2 factory,
board proof, rematch, sequencing and recovery contracts remain supported.
Concurrent matches with different choices retain independent authority state.

## Real browser journey and acceptance

CreateBoard exposes three separate profile selectors and a secondary advanced
section for rack size, bingo bonus, exchange minimum, scoreless completion and
player limits. Exact numeric edits and all profile choices persist through
validation failures; changing one selector never silently changes another.
Interface language changes only interface text. Fresh creation remains untimed
under Atlas 3.11; clock controls belong to #99.

Independent native and production-like browser acceptance must demonstrate two
actual simultaneous matches with different compatible configurations, ordinary
joins/start/gameplay, incompatible/no-allocation failures, retry conflicts,
source identities, changed advanced behavior, and restart/replay continuity.
All source selection effects must reach the canonical engine. Full applicable
validation and actual public deployment verification are required before
claiming this user-facing outcome complete.

Custom word-list libraries (#97:1.8/1.16/1.18/1.20), private tile libraries
(#100:1.27/1.29), custom board libraries (#101:1.33/1.35/1.37), multi-letter tiles
(#98:1.21/1.23), actual hex mechanics (#96:1.4/1.10–1.15), and clocks (#99:3.1/
3.4–3.11) remain their owning scopes. This predefined selector does not claim
those journeys available. #94 retains any original acceptance not delivered by
this bounded slice; it may not close from a catalog or backend test alone.
