# Hexagonal browser and score-analysis boundary

Implementation boundary for #96 and Atlas Decisions 1.13, 1.15 and 1.43. The
independent behavior contract remains [hexagonal-browser.md](../specs/hexagonal-browser.md).

The server composes board shape, premium policy and main-word length metric
through the existing immutable board-profile boundary. Square profile
fingerprints retain their original input tuple. Hex fingerprints additionally
bind the geometry and both scoring choices. Recovery reconstructs these same
inputs instead of substituting square defaults.

`GET /profiles` retains the strict square catalog shape. A configured Hex default
is projected to an actual retained square board with compatible selected words,
tiles and settings, and an accurately recomputed `legacyCompatible` flag. A
Hex-only operator catalog returns HTTP 406 from this legacy endpoint; it remains
fully available through `GET /profiles/hex`, which preserves configured choices
and defaults. No unconfigured classic profiles are invented. The fallback catalog
retains the original classic default identities and offers independent Hex sample
board choices under both premium policies and both length metrics. Center and
interior word premiums let later crossing moves demonstrate policy differences. The browser tries the
extended endpoint and falls back to the original endpoint on HTTP 404. Deploy
the compatible client before exposing Hex configurations on the server. Existing
single-letter public tile defaults stay unchanged; combined-face public presets
remain the separate #121 scope.

Rust evaluates tentative placements by preparing the same complete atomic
candidate as commit, then discarding it. Commit alone publishes the candidate.
Score representability, cumulative totals, final adjustments and clock checks
therefore share one authority with placement, dictionary and scoring validation. Hex public previews include either a native
score breakdown or a bounded structured invalid-placement reason. Browser copy
translates the reason; native errors do not serialize storage diagnostics. The
browser accepts an evaluation only for the current player, turn and exact draft
tiles. Editing or cancelling the draft invalidates that correlation. Commit
remains server-authoritative.

A valid preview includes ordered word text, directions, individual scores,
length metric, bingo bonus and total. Committed Hex history retains the same
native information compactly: its ordered `(direction, score)` pairs reference
`TurnAction::Commit.words`, and the total remains `move_score`. The browser zips
these fields for presentation; it neither selects the main word nor calculates
scores. History retention is unchanged. Optional analysis is omitted for square
matches, preserving their previous serialized bytes. Canonical evidence version
and legacy recovery compatibility remain unchanged.

React projects the axial storage coordinates into pointy-top Hex cells and
renders only on-board cells. Shared-board geometry fills the available display
column; phone controls keep their 44-pixel targets. Keyboard navigation skips
off-board storage coordinates. These are presentation and interaction rules;
legality along all three axes remains entirely native.

Local integration and wire-capacity evidence do not establish public deployment
acceptance. #96 remains open until the deployed client/server journey is verified.
