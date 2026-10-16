# Authoritative match clock model

This headless enabler of #99 (issue #112) gives the authoritative
`scrabble-game` engine an optional, immutable **time control** and a
deterministic **clock state**. It implements Atlas Decisions 3.1, 3.4 and 3.12 (see [Forfeit](#forfeit)), 3.5, 3.8, 3.9 and 3.11. Decisions
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
6. Apply the selected pass or forfeit consequence.

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

## Forfeit

Atlas Decision **3.12** (merged in `d701266`, Answers historical Question 3.2)
settles the provisional #112 restriction. At expiry, remove only the expired
player from participation and turn rotation; freeze their score and private rack,
keep their tiles out of play, and rank them below every remaining player regardless
of score. Continue with the next participating seat while two or more remain;
the last remaining player wins. This applies to two, three and four players.
Ordinary scoreless/going-out completion must exclude forfeited racks from final
deductions/transfers and exclude forfeited players from winners. Duplicate or stale
timeouts cannot apply another elimination. Replaying the same events/instants must
reproduce the elimination, remaining rotation and final result canonically.

The earlier `ForfeitUnavailable` behavior is historical #112 scaffolding and is
superseded by #99; a hosted creation boundary must admit valid forfeit settings.
No new clock defaults, pause controls or scoring policy among forfeited players
are established by this change.

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
    // Optional public-only session projection; omitted when absent.
    pub last_timeout: Option<ClockTimeout>,
}
#[serde(deny_unknown_fields)]
pub struct PlayerClock { pub player_id: PlayerId, pub remaining_ms: u64 }
```

`ClockTimeout` contains `player_id`, `turn` and `consequence` (`pass`/`forfeit`).
It records the most recent successful timeout, remains through later chosen turns,
and is reconstructed by replay. It is not stored in canonical engine evidence:
existing automatic-pass journals therefore remain compatible. It is absent before
any timeout and omitted when absent. See [ADR 0011](../adr/0011-match-clock-presentation-and-forfeits.md).

The clock DTOs derive `Clone, Debug, Eq, PartialEq, Serialize, Deserialize`. `clock` is
`None` for untimed matches, so their snapshot JSON is unchanged. For a timed
match it is `Some` in every phase. In the lobby it lists the seated players with
their starting balance and no instants. In `Finished` it keeps the final
balances and has no instants. The instant fields serialize as `null` when
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
- forfeit elimination, frozen racks/scores and 2–4-player completion (#99);
- canonical serialization, reconstruction and replay round-trip; public clock
  snapshot shape and privacy;
- untimed byte-identity of ruleset JSON, canonical bytes and public and player
  snapshots, pinned against values captured on the pre-#112 baseline `ebf2095`.

The existing `scrabble-game` suites stay green. The full gate is
`bun run check`.

## Historical follow-ups

The provisional forfeit question is resolved by Atlas 3.12. Automatic pass may
retain the ordinary `TurnAction::Pass` history action: #99 must make the timeout
result visible, but this does not require a separate action variant. See the #99
acceptance handoff in [server-match-clock.md](server-match-clock.md#issue-99-independent-acceptance-handoff).
