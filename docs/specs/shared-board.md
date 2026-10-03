# Shared public board

`SharedBoard` renders exclusively from a parsed public snapshot plus the selected ruleset's public presentation data. It has no gameplay store or private-rack dependency. The ruleset supplies board size, row-major premiums, and letter values; a committed blank uses its represented letter with value zero. Native read-only table headers and cell labels expose coordinates, premium markings, letters, values, and blank status without creating hundreds of keyboard tab stops.

The board occupies the main display area. A compact adjacent status area shows phase, active player/turn, roster and scores, and remaining tiles. The layout stacks on narrow viewports while keeping board coordinates and square geometry stable. Premium kinds use distinct semantic theme colors and text abbreviations, so color is not their only signal.

Until transport integration (#8), the entry point is explicitly a board preview. `/scrabble/` shows the compact lobby; `?fixture=playing` and `?fixture=finished` exercise the other view states. These are presentation fixtures, not live matches or an alternate gameplay implementation. The finished phase is a synthetic presentation case until #25 implements authoritative game completion.

`crates/scrabble-game/examples/public_fixtures.rs` generates the committed JSON compatibility fixture from Rust public snapshots and ruleset data. The playing board and represented blank come from actual owned tile placement and authoritative scoring storage. An example test compares the committed JSON against current Rust serialization without rewriting it. After an intentional contract/fixture change, run `bun run fixtures:generate` into a temporary file, replace `apps/web/src/fixtures/public.json`, and format/review the change.

Frontend schemas use strict objects and derive TypeScript types from runtime validation. Unknown private fields and malformed identifiers fail parsing. This is public-shape compatibility evidence; server-side disclosure protection remains the responsibility of scoped snapshots and the later transport/privacy acceptance tests.

`bun run check` now includes two Chromium layout tests. They cover every phase, stable square coordinates as tiles appear, distinct premium colors, represented blanks, read-only semantics, runtime errors, and narrow-screen overflow. Screenshots for all phases are written under ignored `test-results/`; failures retain screenshots and traces. The launcher selects one disposable OS-assigned port for the server and all worker processes.
