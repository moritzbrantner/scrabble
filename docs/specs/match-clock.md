# Authoritative match clock model

This headless enabler of #99 (issue #112) gives the authoritative
`scrabble-game` engine an optional, immutable **time control** and a
deterministic **clock state**. It implements Atlas Decisions 3.1, 3.4 (pass
only, see [Forfeit](#forfeit-provisional)), 3.5, 3.8, 3.9 and 3.11. Decisions
3.6 and 3.7 are satisfied by the server supplying the right instants: the engine
never reads the wall clock and only consumes elapsed time that the caller states
explicitly. Server timers, outage freezing, persistence/recovery wiring,
protocol messages, browser UI and public acceptance stay in #99.

## Time control in the effective `Ruleset`

`Ruleset` gains one immutable field:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub time_control: Option<ruleset::TimeControl>,
```

`None` is an **untimed** match, the default (Decision 3.11). An untimed ruleset
serializes exactly as before #112. Its JSON, canonical state bytes and public
and player snapshots are byte-identical to the pre-#112 engine, the same way the
default hex fields stay omitted in [hexagonal-board.md](hexagonal-board.md).
The canonical evidence version stays **3**.

```rust
#[serde(deny_unknown_fields)]
pub struct TimeControl {
    pub clock: ClockModel,
    pub on_expiry: ExpiryConsequence,
}

#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClockModel {
    /// Decision 3.5 (1): one carried budget per player.
    Cumulative {
        initial_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        increment_ms: Option<u64>,
    },
    /// Decision 3.5 (2): a fresh countdown for every turn.
    PerTurn { allowance_ms: u64 },
}

#[serde(rename_all = "snake_case")]
pub enum ExpiryConsequence { Pass, Forfeit }
```

All three types live in `ruleset` and derive `Clone, Copy, Debug, Eq,
PartialEq, Serialize, Deserialize`. Example JSON:

```json
{"clock":{"kind":"cumulative","initial_ms":60000,"increment_ms":5000},"on_expiry":"pass"}
{"clock":{"kind":"per_turn","allowance_ms":20000},"on_expiry":"forfeit"}
```

### Validation

`Ruleset::validate()` returns `ContractError`, and therefore `GameState::new`
returns `StateError::InvalidRuleset`, when:

- any configured duration (`initial_ms`, `increment_ms` when present,
  `allowance_ms`) is `0` or greater than `ruleset::MAX_CLOCK_MS`
  (`86_400_000`, 24 hours). "No increment" is spelled `None`, never `Some(0)`.
- the clock is `Cumulative` and `on_expiry` is `Pass` but `increment_ms` is
  `None`. This is Decision 3.8, enforced at the authoritative creation boundary.

Cumulative + `Forfeit` without an increment is valid (Decision 3.8). Per-turn
clocks never need an increment. The 24-hour bound is a routine safety limit,
not a product default. No default duration, model or consequence is chosen.

## Injected authoritative time

Every instant is a `u64` count of authoritative milliseconds supplied by the
caller (the server). The engine assumes only that instants never decrease
within a match. It never reads a clock.

An untimed match ignores instants. On a timed match, every operation that starts
or ends a turn needs an instant:

| Untimed entry point                      | Timed entry point                                                                |
| ---------------------------------------- | -------------------------------------------------------------------------------- |
| `GameSession::apply`                     | `GameSession::apply_at(player, sequence, &envelope, now_ms)`                     |
| `GameSession::start_by_authorized_board` | `GameSession::start_by_authorized_board_at(player, sequence, &envelope, now_ms)` |
| `GameState::deal_initial_racks`          | `GameState::deal_initial_racks_at(now_ms)`                                       |
| none                                     | `GameSession::timeout_at(expected_turn, now_ms)`                                 |

`apply_at` and the other `_at` operations behave exactly like their untimed
counterparts on an untimed match, with byte-identical results. On a timed
match the untimed entry points cannot bypass the clock:

- `apply` rejects `Start`, `Preview`, `Commit`, `Pass` and `Exchange` with
  `CommandError::State(StateError::Clock(ClockError::InstantRequired))`.
  `SetName` in the lobby still works.
- `start_by_authorized_board` and `deal_initial_racks` reject with the same
  `ClockError::InstantRequired`.
- `commit::apply` and `commit::apply_with_dictionary` reject with
  `CommitError::State(StateError::Clock(ClockError::InstantRequired))`. They
  check this right after the active-player check and before any placement or
  dictionary validation.

A rejection never mutates state or consumes a sequence.

```rust
// state.rs
pub enum StateError { /* … */ Clock(ClockError) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    InstantRequired,    // timed match driven through an untimed entry point
    TimeRegression,     // instant earlier than the active turn's start
    Expired,            // a turn command at or after the expiry instant
    NotExpired,         // a timeout before the expiry instant
    Untimed,            // a timeout on an untimed match
    ForfeitUnavailable, // provisional: a timeout whose consequence is forfeit
}
```

## Clock state

A timed `GameState` holds, privately:

- one **remaining** balance (ms) per seated player, in seat order;
- the **active turn start** instant while the phase is `Playing`.

When the match starts (`Start` / `deal_initial_racks_at`) every player's
balance is `initial_ms` (cumulative) or `allowance_ms` (per-turn). Then the
first turn begins at the start instant.

**Turn start.** Whenever a turn begins for player P at instant `t` (the start,
or the turn after an accepted commit, pass, exchange or timeout that does not
finish the match), the active turn start becomes `t`, and:

- cumulative: P's balance increases by `increment_ms`, if configured. The
  grant applies on every turn, including P's first and including when P's
  carried balance is zero (Decision 3.8).
- per-turn: P's balance is set to `allowance_ms`.

**Expiry instant.** While a timed match is `Playing`,
`expires_at = active turn start + active player's balance`.
`GameState::clock_expires_at() -> Option<u64>` returns it, and `None` for
untimed matches and outside `Playing`. The active player's stored balance is
the balance at turn start, after the grant. It is not decremented live.

**Turn commands.** `apply_at` validates game, identity, sequence, phase,
expected turn and the active player exactly as `apply` does. For a `Playing`
command it then checks the clock before any command-specific validation:

1. `now_ms < active turn start` → `ClockError::TimeRegression`.
2. `now_ms >= expires_at` → `ClockError::Expired`. A move at the exact expiry
   instant is too late, because the timeout is valid at that instant.

`Preview` passes these checks and changes no clock. An **accepted** commit,
pass or exchange at `now_ms` ends the turn:

- cumulative: the balance becomes `balance − (now_ms − turn start)`, which is
  always positive;
- per-turn: unused time is discarded, and the balance returns to
  `allowance_ms`;

and the next turn, if any, begins at `now_ms`. A **rejected** command (clock,
placement, dictionary, exchange, stale or overflow error) leaves balances, the
turn start, canonical bytes and the sequence unchanged.

**Timeout.** `GameSession::timeout_at(expected_turn, now_ms)` is the explicit
expiry transition, issued by the server timer. It is not a player command and
consumes no sequence. Checks, in order:

1. untimed match → `ClockError::Untimed`;
2. phase not `Playing` → `StateError::WrongPhase`;
3. `expected_turn` is not the current turn → `CommandError::StaleTurn`. This is
   how a duplicate or late timeout for an already-ended turn is rejected;
4. `now_ms < active turn start` → `ClockError::TimeRegression`;
5. `now_ms < expires_at` → `ClockError::NotExpired`;
6. `on_expiry` is `Forfeit` → `ClockError::ForfeitUnavailable` (provisional).

With `Pass`, the timeout applies **automatic pass exactly once** through the
existing turn completion (`finish_turn` / `complete_turn` with move score 0).
It publishes one `PublicTurn` with `TurnAction::Pass {}` for the expired player,
increments the consecutive scoreless counter, can finish the match with the
usual rack deductions, and clears the preview. The expired player's balance
becomes `0` (cumulative) or `allowance_ms` (per-turn). If the match continues,
the next turn begins at `now_ms`, the instant the timeout was applied, not the
earlier expiry instant. No player is charged for time before their turn was
published. Any error leaves the state byte-identical.

All clock arithmetic is checked. An overflowing balance or expiry instant
rejects the transition with `StateError::CounterExhausted` and publishes
nothing.

## Forfeit (provisional)

Decision 3.4 makes forfeit a valid expiry consequence. How a forfeit resolves
in a match of more than two players is undecided (#99, needs-decision). Until
the owner decides:

- forfeit configurations are represented, serialized and validated as above;
- moves on a forfeit-configured match follow the same clock rules, so they are
  rejected with `Expired` at or after expiry;
- `timeout_at` at or after expiry returns `ClockError::ForfeitUnavailable`
  and changes nothing. The engine applies no forfeit outcome.

A forfeit-configured match therefore cannot progress past an expiry. The
server (#99) must not offer forfeit to hosts until the semantics are decided
and this section is replaced.

## Serialization, replay and snapshots

**Canonical evidence.** A timed `GameState::canonical_bytes()` carries the
ruleset's `time_control` and an additional `clock` member holding the balances
and the active turn start. An untimed state omits both keys. Replaying the same
seed with the same `apply_at` / `timeout_at` calls and instants yields
byte-identical canonical state and snapshots, including from a ruleset
deserialized from canonical or snapshot JSON.

**Public snapshot.** `PublicSnapshot.configuration` already carries the full
ruleset, including `time_control`. `PublicSnapshot` gains:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub clock: Option<protocol::PublicClock>,

#[serde(deny_unknown_fields)]
pub struct PublicClock {
    pub players: Vec<PlayerClock>,         // seat order
    pub turn_started_at_ms: Option<u64>,   // Some only while Playing
    pub expires_at_ms: Option<u64>,        // == GameState::clock_expires_at()
}
#[serde(deny_unknown_fields)]
pub struct PlayerClock { pub player_id: PlayerId, pub remaining_ms: u64 }
```

Both derive `Clone, Debug, Eq, PartialEq, Serialize, Deserialize`. `clock` is
`None` for untimed matches, so their snapshot JSON is unchanged. For a timed
match it is `Some` in every phase. In the lobby it lists the seated players with
their starting balance and no instants. In `Finished` it keeps the final
balances and has no instants. The option fields serialize as `null` when
absent. Clients compute a live countdown as `expires_at_ms − now`. The clock
contains no rack, bag, seed or other private data. `player_snapshot().public`
equals `public_snapshot()`.

## Acceptance

`cargo test -p scrabble-game --test match_clock --locked` was authored
independently before implementation. It uses only the public engine API with
injected instants and covers:

- configuration validation: bounds, `Some(0)`, the Decision 3.8 rejection, the
  valid forfeit combinations, JSON shape, round-trip and unknown fields;
- cumulative carry-over and turn-start increment, including the first turn and
  the zero-balance grant;
- per-turn reset with unused time discarded;
- deduction on commit, pass and exchange; a rejected commit and a time
  regression leave the clock and sequence unchanged;
- automatic pass on expiry exactly once, the expiry boundary, rejection of
  moves after expiry, and stale or duplicate timeouts;
- interplay with the scoreless-turn ending and frozen finished matches;
- untimed entry points rejected on timed matches;
- provisional forfeit blocking;
- canonical serialization, reconstruction and replay round-trip; public clock
  snapshot shape and privacy;
- untimed byte-identity of ruleset JSON, canonical bytes and public and player
  snapshots, pinned against values captured on the pre-#112 baseline `ebf2095`.

The existing `scrabble-game` suites stay green. The full gate is
`bun run check`.

## Open questions (owner)

1. **Forfeit semantics** (#99 comment, options A/B/C). Applying forfeit is
   blocked by contract until answered.
2. **Distinct history marker for an automatic pass.** The timeout records an
   ordinary `TurnAction::Pass {}`, which keeps the public DTO unchanged. If the
   #99 UI must tell a timed-out pass from a chosen pass, a new `TurnAction`
   variant is a protocol change that needs a decision.
