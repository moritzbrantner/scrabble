# Server-authoritative match clocks

This headless server enabler of #99 (issue #114) runs **timed matches** in
`crates/scrabble-server` on top of the merged engine clock
([match-clock.md](match-clock.md), #112). It implements Atlas Decisions 3.1,
3.4, 3.5–3.9 and 3.11–3.12 at the
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

## Forfeit

Atlas 3.12 settles elimination and continued multiplayer play. Valid forfeit
creation must be accepted. The hosted expiry loop applies the engine's forfeit
transition once on the first tick reaching expiry, publishes the result to public
board and private controllers, and preserves elimination through replay/recovery.
Player disconnect does not prevent expiry; a real server outage freezes time.
The historical #114 `forfeit-unavailable` rejection is superseded by #99.

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

| Request                                                                | Response                                                            |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------- |
| malformed `timeControl` (unknown kind/field, wrong type, negative)     | `400 {"error":"invalid-request"}`                                   |
| any duration `0` (including `increment_ms: 0`) or above `MAX_CLOCK_MS` | `400 {"error":"invalid-time-control","reason":"invalid-duration"}`  |
| `cumulative` + `pass` without `increment_ms` (Decision 3.8)            | `400 {"error":"invalid-time-control","reason":"missing-increment"}` |
| any valid `on_expiry: "forfeit"`                                       | Accepted under Atlas 3.12 (#99)                                     |

The checks validate duration and then Decision 3.8. A cumulative forfeit clock
without increment is valid; both models support forfeit under Atlas 3.12.

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

1. **Forfeit semantics** (#99). Resolved by Atlas 3.12; see the acceptance handoff below.
2. **Distinct automatic-pass marker** (match-clock.md, open question 2). The
   automatic pass publishes an ordinary `TurnAction::Pass {}`.
3. **Tick granularity.** Expiry is applied at 50 ms tick boundaries, so a
   player can gain up to 49 ms beyond an off-grid allowance. This is assumed to
   be acceptable. If exact sub-tick fairness is required, the owner must decide
   it.
4. **Deployment ruleset clocks.** A `SCRABBLE_RULESET_FILE` containing
   `time_control` would make every v1/v2 match timed. This contract does not
   cover it. It is treated as unsupported operator input until decided.

## Issue 99 independent acceptance handoff

Authority is `docs/DECISION-ATLAS.md` Decisions **3.1, 3.4–3.9, 3.11–3.12**, priority
**4.1**, at baseline **d7012663714d79c17f173de11ebdd70564ba2235**; #99 retains the
complete product scope. A distinct acceptance agent authored this contract before
production changes and without an implementation patch. Shared policy
`sourceRevision`: **5ad0db36974c8b0e5f9812c90ee34a4f227852c3**.

Every named new or reused suite below is required at the final PR head. There is
no verified complete impact graph, so run the full applicable `bun run check`
before merge, then published-service acceptance after documented deployment.

- **Explicit setup and fresh untimed creation (3.1/3.11):**
  `apps/web/browser/match-clock.spec.ts` starts with clocks off, checks newly
  enabled model/consequence/durations have no invented selections, creates timed
  then fresh untimed in the same browser, and checks actual request payloads.
  Enabling cumulative automatic pass with absent/zero increment visibly rejects
  before any POST (3.8). Existing `profile-creation.spec.ts` preserves independent
  profiles/advanced gameplay settings; clocks remain separate from preferences.
- **Both models and both outcomes (3.4/3.5):** the four browser cases use the
  actual React host form, separate player contexts, native service and real time;
  board/controller show the selected rules and clocks. The active accessible timer
  changes while the inactive timer stays fixed. Actual timeout changes the legal
  turn or produces the last-survivor result, with a visible timeout history result.
  These tests add no browser clock override or fake native game. Inputs 2+1 seconds
  cumulative / 3 seconds per turn are test data, not approved defaults.
- **Transitions, cumulative carry, first/zero-bank grants, per-turn reset,
  accepted commit/pass/exchange, invalid/rejected commands, exact boundary and
  timeout race/retry:** reuse **all** `scrabble-game/tests/match_clock.rs` cases,
  including independent #112 cases already present on baseline d701266; the
  provisional forfeit case is replaced under 3.12. Instant injection stays at the
  existing public native seam and establishes deterministic ordering without
  wall-clock CI assumptions.
- **2–4-player forfeit and completion (3.12):** new engine cases exercise both
  clock models, a forfeiter with a higher score, frozen private rack/score, bag
  conservation, skipped seats after a full live rotation, continued multiplayer
  play, duplicate rejection and last-survivor win. Separate cases cover scoreless
  completion and going-out transfers excluding forfeited racks. Replay from the
  same initial seed/events matches canonical elimination evidence.
- **Authoritative boundary and immutable identity (3.1/3.8):** reuse/update
  `server_process.rs::timed_creation_validates_time_controls_without_allocation_and_conflicts_on_retry`
  for impossible settings, malformed input, no allocation, same-key retry and
  conflict. New `timed_forfeit_creation_is_valid_and_the_real_server_finishes_once`
  creates per-turn and cumulative forfeit (with/without increment), observes real
  hosted expiry, frozen rack/scores and identical board/controller result.
- **Disconnect/outage, retry and replay (3.6/3.7):** reuse **all**
  `scrabble-server/tests/match_clock_server.rs` and the real process
  `timed_board_match_auto_passes_from_the_real_tick_loop_and_freezes_across_restart`.
  New native forfeit cases verify 2-player result and 3–4-player continued rotation,
  canonical replay, recovery of elimination without duplication, continued clocks
  for disconnected survivors, and stale player commands. Browser cases reload the
  expired controller and retain the authoritative outcome. No pause control is
  exposed (3.9); no new pause command is authorized.
- **Privacy/compatibility:** reuse clock suite public/private projections and
  pinned untimed byte-identity cases, existing `private_boundaries.rs`, v1/v2 strict
  creation cases and browser creation/reconnect suites. New browser cases inspect
  actual datagrams through the existing observer, prove the board rack is empty,
  and retain private player racks across expiry/reload.
- **Accessibility/layout/performance:** new browser cases run axe on board and
  phone and attach desktop/390px phone screenshots. Reuse existing accessibility,
  preferences, playable-loop and performance-budget contracts, including the 50ms
  interaction requirement. `match-clock.spec.ts` is explicitly included in the
  production-like Pages browser selection as well as the development selection.
- **Architecture/resource ownership:** existing server tick, replay, private
  projection and strict protocol contracts remain required. Independent source
  review before merge must verify Rust owns authoritative clock transitions,
  React only presents authoritative time, settings cannot mutate mid-match, no
  manual pause command/UI exists, and no unbounded timer/polling per unrelated
  match was added. Those source-review obligations are not claimed as executed
  architecture tests.
- **Publication and physical devices:** after local/full gates, use the documented
  native Hetzner publisher before Pages where needed, preserve active games, then
  inspect the actual https://moritzbrantner.github.io/scrabble/ route against the
  reachable authoritative service: untimed after timed creation, both modes and
  both outcomes, independent devices, remaining time and legal completion. No
  test-only duration override is evidence of product defaults. Desktop/phone
  Playwright are browser evidence; physical-device testing remains a separate
  release obligation under Atlas 4.1 and #32/#34. Missing hosting/device access
  must be recorded explicitly; none is claimed complete in this handoff.

### Initial evidence

Focused baseline commands ran with the disposable target wrapper
`python3 ~/.local/share/coding-agent/disk-maintenance.py run -- …`:

- `cargo test -p scrabble-game --test match_clock forfeit --locked`: **exit 101**,
  two executed new forfeit cases fail at `ForfeitUnavailable`.
- `cargo test -p scrabble-server --test match_clock_server forfeit --locked`:
  **exit 101**, expected Finished(last survivor), observed Playing(first player).
- `cargo test -p scrabble-server --test server_process timed_forfeit_creation --locked`:
  **exit 101**, real API returns 400 `forfeit-unavailable`, expected successful creation.
- `bun scripts/test-browser.ts match-clock.spec.ts --grep 'each fresh creation'`:
  **exit 1**, actual served frontend has no accessible Enable clocks checkbox;
  screenshot/trace retained in `test-results/match-clock-each-fresh-cre-5561a-invents-no-timed-selections/`.
- `bun run typecheck` and `bun run lint:web`: passed for the authored browser contract.

Full baseline transcript: `/tmp/scrabble99-acceptance-red.log` (local, disposable
supporting evidence). The final going-out and multiplayer native recovery
extensions were added after those focused red runs and have **not yet executed**;
all four timed browser cases, Pages run, full suites, publication and devices are
also **pending**. Earlier #112/#114 evidence is historical, not a new pass claimed
by this handoff. Required final commands include both complete native clock suites,
all `server_process` cases, the complete browser clock file in both development and
Pages configurations, and the repository full gate.
