# Match clock presentation and forfeits

Status: accepted implementation of Atlas 3.1, 3.4–3.9, 3.11–3.12 (#99).

Rust remains the single time and turn authority. The server's existing 20 Hz
simulation tick supplies match time, applies either timeout consequence and
journals the transition. No new scheduler, per-match polling service, browser
command or manual pause boundary is introduced. Expiry remains quantized to the
existing tick boundary. A forfeit is an atomic candidate-state transition:
retain the seat/rack, mark participation ended, skip that seat, and finish when
only one participant remains. Ordinary completion excludes forfeited racks from
both deductions and transfers, and excludes forfeited players from winners.

`forfeited: true` is an additive public player field, omitted when false. The
private participation flag is also omitted when false, preserving canonical
bytes for prior games. `TurnAction::Forfeit` records the accepted elimination.
Previously hosted forfeit creation was rejected, so this does not reinterpret
old forfeit histories. Existing untimed and automatic-pass histories keep their
canonical representation and replay semantics.

The optional public `clock.last_timeout` identifies the most recent authoritative
timeout's player, turn and consequence. It lets the board and controller explain
automatic pass without changing ordinary `TurnAction::Pass` history. The session
reconstructs this presentation projection by replaying the existing timeout ticks;
it is deliberately excluded from canonical engine bytes, like the existing
preview projection, so adding the display does not invalidate old pass journals.
It changes only after a successful timeout. Finished matches retain it.

React accepts the immutable time-control configuration and clock projection.
Countdowns are estimates anchored to the actual snapshot tick and the admitted
server tick rate, extrapolated using browser monotonic time. A display reaching
zero cannot change the turn, freeze the server, apply an outcome or replenish a
balance. Subsequent snapshots/reconnects replace the estimate. Rendering updates
are confined to the clock component's owned timer and cleaned up on unmount or
completion. Required clock settings remain blank until the host chooses them;
every new creation flow starts untimed, without stored clock presets.

Publication order is native service first, then Pages. Existing untimed games and
recovery bundles must survive the documented graceful deployment; clock-capable
Pages should connect only after the service accepts both outcomes. Browser and
native acceptance are recorded separately from public and physical-device release
evidence on issue #99.
