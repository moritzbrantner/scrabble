# Confirmed pass turns

The active connected phone can pass without committing placements. **Pass turn** opens an inline confirmation; **Keep playing** retains the draft and returns focus, while **Confirm pass** sends the authenticated current-turn command. Both actions use touch targets of at least 44 pixels. A confirmed submission stops preview heartbeats before reserving the pass sequence. Delivery is not acceptance: controls wait for the authoritative snapshot, and failed/unconfirmed delivery keeps the draft.

The existing native `GameSession::apply` pass operation validates game, identity, sequence, phase, active player and expected turn before advancing once and clearing its preview. Board, score, bag order and both racks stay unchanged. Repeated transport sequences are ignored; a retry with a new sequence but the old turn is rejected. Native counter exhaustion is atomic. Scoreless end-game handling follows in #25.

Real-runtime tests compare complete canonical board, bag and player data, preserve previews after rejected passes, fence inactive/stale retries, prove exactly-once advancement, and verify replay and recovery hashes. The built Pages scenario stages a draft, cancels confirmation without changing the turn, confirms pass, checks all-client convergence and unchanged rack identities, clears both previews, and disables the previous player's controls.

Full gate: `bun run check`. Shared policy sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
