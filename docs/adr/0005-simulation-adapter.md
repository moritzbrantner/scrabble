# Scrabble simulation adapter

Status: accepted

## Decision

`crates/scrabble-server` owns a library adapter implementing upstream `GameSimulation`. Scrabble state and command semantics remain in `scrabble-game`; the runtime retains session tokens, epochs, transport sequences, reconnect grace, replay capture and serving. Runtime player IDs map losslessly from u32 to the domain's u64 identity and are never allocated or substituted by the adapter.

The adapter opts into `SnapshotScope::PlayerScoped`. Its connection projection serializes `GameSession::player_snapshot` for one currently admitted identity. Its separate canonical snapshot explicitly wraps private `GameState::canonical_bytes` with a sorted admitted identity set and monotonic admission watermark, in adapter evidence format version 1. Domain canonical JSON remains opaque embedded JSON; no second domain representation is implemented. Upstream `SimulationSnapshot::new` hashes the deterministic tick and payload through its existing integrity contract.

Tick advancement increments a checked counter and expires presentation-only previews after forty ticks (two seconds at 20 Hz). Wall-clock tick scheduling cannot deal tiles, rotate turns, or award scores. Preview deadlines are excluded from canonical snapshots; expiry cannot change racks, scores, the board, or turns. The runtime decides when its reconnect grace expires and invokes the fallible player-removal hook.

## Removal policy

An expired/rolled-back lobby admission removes its undealt seat and increments public revision. Existing player IDs and their relative order remain unchanged; the first remaining seat is host. A later admission receives a new runtime ID, and IDs must increase even when called directly through the simulation seam. One u32 admission watermark replaces a growing retired-identity list, so repeated lobby admission/removal cannot grow retained metadata without bound.

A started game's expired player is deactivated for command and private-projection access, but retains its seat, rack, score and committed ownership in canonical state. Its preview is cleared. This preserves conservation and identity. The adapter does not skip a departed active player's turn; reconnect behavior is tracked in #26 and boundary hardening in #27.

The upstream `GameSimulation::connection_changed` hook clears previews immediately after accepted disconnects and reconnects. Invalid tokens and stale epochs do not notify. Runtime presence also hides previews from offline players during recovery, before reconnect invalidates reconstructed presentation. No browser observation is treated as authority.

## Evidence

Browser-free tests cross both `GameSimulation` and the real `MatchRuntime` seams. They establish scoped rack isolation, private canonical completeness, atomic rejected commands, ephemeral previews, tick/turn independence, lobby capacity after expiry, preserved started-game ownership, epoch fencing, sequence continuation, replay checkpoint equality, and recovery with the same ruleset/seed inputs.
