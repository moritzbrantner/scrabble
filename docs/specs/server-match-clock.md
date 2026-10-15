# Server-authoritative match clocks

This headless server enabler of #99 (issue #114) runs **timed matches** in
`crates/scrabble-server` on top of the merged engine clock
([match-clock.md](match-clock.md), #112). It implements Atlas Decisions 3.1,
3.4 (automatic pass only, forfeit stays blocked), 3.5–3.9 and 3.11 at the
authoritative server. Browser creation controls, countdown UI, forfeit outcomes
and public acceptance stay in #99.

Baseline: `1eb8b1b` (main after PR #113). The engine contract is unchanged; this
document only states what the server supplies to it and what clients and
operators can observe.

## Authoritative match time

The server never reads the wall clock for match time. Match time is derived
from the simulation's own deterministic tick:

```text
now_ms(tick) = tick × (1000 / SCRABBLE_TICK_HZ) = tick × 50
```

`SCRABBLE_TICK_HZ` stays `20`, so one tick is exactly 50 ms of match time and
tick `0` is instant `0`. The multiplication is checked: an overflow fails the
operation atomically, like the existing tick exhaustion. The tick is already
part of every `SimulationSnapshot`, replay record and recovery image, so the
canonical evidence needs no new wall-clock or time field. The canonical wrapper
keeps its keys (`version`, `admitted`, `last_admitted_player`, `game`,
`lifecycle`, `board`) and its versions (1 legacy, 2 board). The timed `game`
member carries the engine's `time_control` and `clock` as specified by #112.

Consequences, by construction:

- **Disconnects do not pause (3.6).** Ticks advance while the server runs,
  regardless of any player's connection. `connection_changed`, grace
  disconnects and capability expiry (`try_remove_player` on a started seat) do
  not touch the clock. A seat whose capability expired keeps timing out.
- **Outages freeze (3.7).** Ticks advance only while the runtime runs. A graceful
  shutdown saves the tick; recovery resumes at that tick. Downtime is never
  charged: after recovery the balances, `turn_started_at_ms` and
  `expires_at_ms` are exactly the saved values, and the expiry happens after
  the same number of further ticks as without the outage.
- **No manual pause (3.9).** No command, API or operator control pauses ticks
  for one match.

Wall-clock fields stay unchanged. Lifecycle `created_at`, the six-hour
expiry and the creation retry window still use Unix seconds. Their recovery
semantics are unchanged.

## Commands and start

`ScrabbleSimulation::apply_command` passes `now_ms(current tick)` to the engine:

- every gameplay command goes through `GameSession::apply_at`, including `Start`
  on legacy and version-1 matches;
- the shared board's `Start` goes through
  `GameSession::start_by_authorized_board_at`;
- lobby-only `SetName`/`ClaimBoard` handling is unchanged.

The current tick is the tick of the most recent `advance_tick`. Commands
processed between two ticks share that instant. Untimed matches are unaffected
because the engine's `_at` operations are byte-identical to the untimed ones
for them.

### Clock rejections

A clock error from a command is a **recoverable, private command rejection**
(`SimulationError::command_rejected`). It is delivered only to the submitting
connection, consumes no sequence, appends no replay record and leaves
canonical and projected state unchanged. Its payload follows the existing
`exchange_unavailable` shape:

```json
{
  "version": 1,
  "game_id": "<game id>",
  "expected_turn": "<turn>",
  "error": { "kind": "clock_expired" }
}
```

`ClockError::Expired` maps to `clock_expired`. With the expiry loop below,
`Expired` is only reachable when a timeout cannot be applied, for example in a
forfeit-configured simulation (see [Forfeit](#forfeit-stays-blocked)). The
other clock errors (`InstantRequired`, `TimeRegression`) are server invariant
violations that tick-derived time prevents. They must also leave state
unchanged. Their payload is not part of this contract.

## Automatic pass on expiry

`ScrabbleSimulation::advance_tick` first increments the tick and applies the
existing preview expiry. Then, when the match is `Playing` and
`clock_expires_at()` is `Some(e)` with `now_ms(tick) >= e`, it calls
`GameSession::timeout_at(current turn, now_ms(tick))` **once**.

- The pass is applied on the **first tick whose instant reaches the expiry**.
  When the expiry is not a multiple of 50 ms, the pass happens at the next tick
  boundary, and the next turn begins at that tick's instant (engine rule:
  `timeout_at` starts the next turn at the instant it is applied). Example:
  per-turn allowance 1010 ms, turn started at 0 → no pass at tick 20 (1000 ms),
  pass at tick 21 (1050 ms), next turn starts at 1050 and expires at 2060.
- At most one timeout is applied per tick. A new turn always has a positive
  balance (Decision 3.8 and the per-turn allowance), so it can't already be
  expired.
- The automatic pass is an ordinary accepted transition. It publishes one
  `PublicTurn` with `TurnAction::Pass {}` for the expired player in the next
  player snapshots. A command for the ended turn is then stale.
- When the automatic pass finishes the match (scoreless-turn limit), the
  lifecycle's `finished_at_tick` is that tick, exactly like a command that
  finishes the match.
- A stale or duplicate timeout cannot happen: after the pass the active turn
  has changed and its expiry lies in the future.

### Replay and recovery

The automatic pass is a deterministic consequence of the tick. The upstream
runtime already journals every tick: a checkpoint or digest checkpoint is
recorded after `advance_tick`. Canonical replay (`verify_replay`) re-executes
the same ticks and reproduces the same pass. The checkpoint after the expiry
tick already contains it. No new replay record type and no side journal are
introduced. Recovery restores the state after the pass, so a restart never
applies a second pass for the same turn. This holds even when the recovery
image is taken on the expiry tick itself.

## Forfeit stays blocked

Until the owner decides #99's forfeit semantics:

- the creation API rejects any `on_expiry: "forfeit"` (below), so a hosted
  match can never be forfeit-configured;
- a forfeit-configured `ScrabbleSimulation` built directly (for example in
  tests) stays safe. At or after expiry `timeout_at` returns
  `ForfeitUnavailable`. `advance_tick` still succeeds and changes nothing else.
  Moves are rejected with `clock_expired`. The match can't progress past that
  expiry, as the engine contract requires.

Any other error from `timeout_at` fails the tick atomically. With tick-derived
time this can't happen.

## Creation request

Only the version-3 configured creation request
([predefined-match-setup.md](predefined-match-setup.md)) gains one optional
top-level field:

```json
{
  "version": 3,
  "requestId": "…",
  "requestedAt": 1791070000,
  "selection": { "wordList": {…}, "tile": {…}, "board": {…} },
  "rules": { "rack_size": 7, … },
  "timeControl": {"clock":{"kind":"cumulative","initial_ms":60000,"increment_ms":5000},"on_expiry":"pass"}
}
```

- The `timeControl` value is exactly the engine's `TimeControl` JSON, with
  snake_case inner names, the same convention as `rules`. Unknown fields and
  kinds are rejected.
- An absent field or `null` means **untimed** (Decision 3.11). There is no
  default duration, model or consequence.
- The effective ruleset's `time_control` is the requested value. It is part of
  the immutable composition, the composed ruleset identity and the persisted
  initialization manifest. A timed match's public `ruleset` identity therefore
  differs from an otherwise identical untimed match's. Untimed requests produce
  byte-identical rulesets, identities and manifests to the pre-#114 server.
- Version-1 and version-2 requests are unchanged. They are strict, so a
  `timeControl` member is `400 invalid-request`. A host that enables a clock
  uses version 3.

### Validation and errors

Validation happens before any initialization or allocation. Every rejection
leaves the hosted-match count unchanged and binds nothing to the creation key.
A later valid request with the same `requestId` succeeds.

| Request                                                                | Response                                                              |
| ---------------------------------------------------------------------- | --------------------------------------------------------------------- |
| malformed `timeControl` (unknown kind/field, wrong type, negative)     | `400 {"error":"invalid-request"}`                                     |
| any duration `0` (including `increment_ms: 0`) or above `MAX_CLOCK_MS` | `400 {"error":"invalid-time-control","reason":"invalid-duration"}`    |
| `cumulative` + `pass` without `increment_ms` (Decision 3.8)            | `400 {"error":"invalid-time-control","reason":"missing-increment"}`   |
| any `on_expiry: "forfeit"` that is otherwise valid                     | `400 {"error":"invalid-time-control","reason":"forfeit-unavailable"}` |

The checks run in this order: duration, then 3.8, then forfeit. A cumulative
forfeit clock without increment is valid under 3.8, so it is rejected only as
`forfeit-unavailable`.

Idempotent retries compare the time control like any other effective setting.
An identical retry returns the same `JoinInformation`. Reusing the key with a
different, added or removed time control returns `409 creation-conflict`.

### Recovery of the configuration

A restart reconstructs the timed effective ruleset from the authenticated
manifest. The public configuration's `time_control` is unchanged after
recovery, and canonical replay through `MatchFactory::create` verifies.

## Untimed compatibility

Untimed matches (all v1/v2 matches, configured IDs and v3 without
`timeControl`) are byte-compatible:

- canonical snapshots, player snapshots, replay logs and recovery images are
  byte-identical to the pre-#114 server for the same inputs and ticks;
- recovery images written by the pre-#114 server restore and verify unchanged;
- public snapshots have no `clock` member and the configuration has no
  `time_control` member.

## Acceptance

Independently authored before implementation (acceptance context, #114):

- `cargo test -p scrabble-server --test match_clock_server --locked`: native
  `MatchRuntime<ScrabbleSimulation>` tests with real `advance_tick` calls,
  `verify_replay`, `freeze_for_recovery`/`recovery_image`/`restore_from_recovery`,
  runtime `disconnect`/`reconnect` and grace expiry. They cover tick-derived
  start and command instants, cumulative/per-turn deduction, automatic pass
  exactly once at the expiry tick (including off-grid expiries and zero-balance
  grants), finishing by automatic pass, replay/recovery without duplication,
  outage freeze across a real wall-clock gap, disconnect not pausing, structured
  `clock_expired` rejections and the blocked forfeit tick, and pinned untimed
  byte identity.
- `cargo test -p scrabble-server --test server_process --locked timed_`: the
  real executable over HTTP and WebTransport. It covers version-3 timed
  creation for both models, the rejection table without allocation, retry
  conflicts, untimed defaults and identity separation, a board-started timed
  match whose clock runs from the real tick loop and auto-passes, and graceful
  shutdown with a downtime longer than the allowance, followed by recovery with
  the same clock and no extra pass.

The existing `scrabble-server` suites stay green. The full gate is
`bun run check`.

## Open questions (owner)

1. **Forfeit semantics** (#99). Creation rejects forfeit until answered.
2. **Distinct automatic-pass marker** (match-clock.md, open question 2). The
   automatic pass publishes an ordinary `TurnAction::Pass {}`.
3. **Tick granularity.** Expiry is applied at 50 ms tick boundaries, so a
   player can gain up to 49 ms beyond an off-grid allowance. This is assumed to
   be acceptable. If exact sub-tick fairness is required, the owner must decide
   it.
4. **Deployment ruleset clocks.** A `SCRABBLE_RULESET_FILE` containing
   `time_control` would make every v1/v2 match timed. This contract does not
   cover it. It is treated as unsupported operator input until decided.
