# Phone tentative move editor

The active connected phone selects a rack tile and taps a destination on a scrollable board. Squares are at least 44 pixels, show coordinates and the configured premium markings, and initially center on the opening square. Tentative tiles use a dashed border and distinct background; committed tiles cannot be overwritten. Choosing a placed tile allows moving it, and **Return selected tile** makes it available again. **Cancel move** clears the draft. These actions never mutate the authoritative rack or committed board.

A blank opens buttons for the ruleset's represented letters, without a text input or keyboard. Choosing a letter completes that tentative placement. Missing/invalid blank assignments, ownership, duplicate tiles/squares and occupied destinations fail locally with explicit feedback. Structural guidance covers one-line placement, gaps, center coverage, word length and connection to existing tiles. It is guidance, not a promise of dictionary acceptance.

The draft context binds the game, authenticated recipient, phase/turn, own rack and committed board. Authoritative changes clear the editor's draft and selection before they can be reused. Presence flags, public scores and ephemeral previews do not invalidate a draft. Waiting/disconnected players cannot place or submit tiles; ordinary connection loss retains the draft for explicit reconnect when the authoritative context remains valid.

**Commit move** requires a structurally valid draft and sends the existing authenticated, sequenced command with the current expected turn. Delivery is not acceptance. Controls wait for authoritative context changes; an unconfirmed submission retains the draft after a five-second deadline. Send failures also retain it. The current runtime closes a connection on simulation command rejection, so the phone's explicit reconnect restores its identity and keeps an unchanged draft. Authoritative commit/scoring changes the board, score, replacement rack and turn together. Preview publication coalesces edits and refreshes the current draft; submission synchronously suspends it before reserving the commit sequence, so an old-turn heartbeat cannot follow a successful commit.

## Evidence

A browser-free Rust oracle generates 1,970 legal and invalid cases across empty, committed and smaller boards. The browser validator agrees with the native structural result, including rejection ordering. Separate draft tests cover moving/removing, blank choices, invalid feedback and safe authoritative reconciliation without modifying snapshots.

Chromium uses the real multiplayer flow to compose horizontal and vertical drafts, move/remove tiles, show line/gap errors, submit a dictionary-rejected commit without changing public state, and reconnect with the draft retained. An isolated browser harness feeds Rust-generated scoped projections to the same production editor to exercise blank controls and reset on authoritative draw/turn updates. That harness is outside the production application import graph. Existing multi-phone coverage continues to verify rack isolation and narrow portrait/landscape layout. Full gate: `bun run check`.

Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
