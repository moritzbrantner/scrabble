//! Independent acceptance contract for server-authoritative match clocks (#114).
//! Specification: docs/specs/server-match-clock.md. Engine contract: docs/specs/match-clock.md.
//!
//! Real `MatchRuntime<ScrabbleSimulation>` with real `advance_tick` calls, native replay
//! verification and recovery images. Match time is `tick × 1000 / SCRABBLE_TICK_HZ` (50 ms).
//! Fixture: two players, racks of two `A` tiles worth 1, a normal board and the word `AA`,
//! so the opening commit at (7, 7)–(7, 8) always scores 2.
use game_server::{
    CommandOutcome, MatchRuntime, ReconnectToken, RecoveryImage, ReplayLog, RuntimeError,
    SessionLease, verify_replay,
};
use scrabble_game::{
    dictionary::WordList,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{
        Command, CommandEnvelope, Phase, Placement, PlayerClock, PlayerSnapshot, PublicClock,
        TurnAction,
    },
    ruleset::{
        ClockModel, ExpiryConsequence, Premium, Ruleset, TileDefinition, TileFace, TimeControl,
        english_fixture,
    },
};
use scrabble_server::simulation::{SCRABBLE_TICK_HZ, ScrabbleSimulation};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

type Runtime = MatchRuntime<ScrabbleSimulation>;

const MS_PER_TICK: u64 = 1000 / SCRABBLE_TICK_HZ as u64;
const GRACE: u64 = 100_000;

fn rules(time_control: Option<TimeControl>, scoreless_turn_limit: u16) -> Ruleset {
    let mut rules = english_fixture();
    rules.maximum_players = 2;
    rules.rack_size = 2;
    rules.bingo_bonus = 0;
    rules.exchange_minimum_bag = 1;
    rules.scoreless_turn_limit = scoreless_turn_limit;
    rules.premiums.fill(Premium::Normal);
    rules.tiles = vec![TileDefinition {
        face: TileFace::Letter { letter: 'A' },
        count: 40,
        value: 1,
    }];
    rules.time_control = time_control;
    rules
}
fn per_turn(allowance_ms: u64, on_expiry: ExpiryConsequence) -> Option<TimeControl> {
    Some(TimeControl {
        clock: ClockModel::PerTurn { allowance_ms },
        on_expiry,
    })
}
fn cumulative(initial_ms: u64, increment_ms: Option<u64>) -> Option<TimeControl> {
    Some(TimeControl {
        clock: ClockModel::Cumulative {
            initial_ms,
            increment_ms,
        },
        on_expiry: ExpiryConsequence::Pass,
    })
}
fn fresh(rules: &Ruleset) -> ScrabbleSimulation {
    let dictionary = Arc::new(WordList::from_text(rules.dictionary.clone(), "AA\n").unwrap());
    ScrabbleSimulation::with_dictionary(GameId::new(1), rules.clone(), [7; 32], dictionary)
        .unwrap()
        .with_lifecycle(1000)
}
fn seated(rules: &Ruleset, grace: u64) -> (Runtime, [SessionLease; 2]) {
    let mut runtime = MatchRuntime::new_with_replay_capture(fresh(rules), grace);
    let first = runtime.admit(ReconnectToken([1; 16])).unwrap();
    let second = runtime.admit(ReconnectToken([2; 16])).unwrap();
    assert_eq!((first.player_id, second.player_id), (1, 2));
    (runtime, [first, second])
}
fn envelope(player: u32, sequence: u32, turn: u64, command: Command) -> Vec<u8> {
    serde_json::to_vec(&CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(u64::from(player)),
        sequence,
        expected_turn: TurnId::new(turn),
        command,
    })
    .unwrap()
}
fn submit(
    runtime: &mut Runtime,
    lease: &SessionLease,
    sequence: u32,
    turn: u64,
    command: Command,
) -> Result<CommandOutcome, RuntimeError> {
    runtime.submit_command(
        lease.player_id,
        lease.connection_epoch,
        sequence,
        &envelope(lease.player_id, sequence, turn, command),
    )
}
fn view(runtime: &Runtime, player: u32) -> PlayerSnapshot {
    serde_json::from_slice(&runtime.snapshot_for(player).unwrap().payload).unwrap()
}
fn clock(runtime: &Runtime, player: u32) -> PublicClock {
    view(runtime, player)
        .public
        .clock
        .expect("a timed match publishes its clock")
}
fn balances(clock: &PublicClock) -> Vec<u64> {
    clock
        .players
        .iter()
        .map(|player| player.remaining_ms)
        .collect()
}
fn advance(runtime: &mut Runtime, ticks: u64) {
    for _ in 0..ticks {
        runtime.advance_tick().unwrap();
    }
}
fn ms(tick: u64) -> u64 {
    tick * MS_PER_TICK
}
fn canonical(runtime: &Runtime) -> Value {
    serde_json::from_slice(&runtime.snapshot().unwrap().payload).unwrap()
}
fn passes(runtime: &Runtime, observer: u32) -> Vec<(u64, u64)> {
    view(runtime, observer)
        .public
        .history
        .iter()
        .map(|turn| {
            assert_eq!(turn.action, TurnAction::Pass {});
            (turn.turn.get(), turn.player_id.get())
        })
        .collect()
}
fn playing(active: u64, turn: u64) -> Phase {
    Phase::Playing {
        active_player: PlayerId::new(active),
        turn: TurnId::new(turn),
    }
}
fn start(runtime: &mut Runtime, lease: &SessionLease) {
    assert_eq!(
        submit(runtime, lease, 1, 0, Command::Start {}).expect("timed start uses the tick instant"),
        CommandOutcome::Applied
    );
}

#[test]
fn match_time_is_the_simulation_tick_and_start_and_commands_use_it() {
    assert_eq!(MS_PER_TICK, 50, "20 Hz ticks are 50 ms of match time");
    let rules = rules(per_turn(20_000, ExpiryConsequence::Pass), 10);
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    let lobby = clock(&runtime, 1);
    assert_eq!(
        lobby,
        PublicClock {
            players: vec![
                PlayerClock {
                    player_id: PlayerId::new(1),
                    remaining_ms: 20_000
                },
                PlayerClock {
                    player_id: PlayerId::new(2),
                    remaining_ms: 20_000
                },
            ],
            turn_started_at_ms: None,
            expires_at_ms: None,
        }
    );
    advance(&mut runtime, 7);
    start(&mut runtime, &one);
    let started = clock(&runtime, 2);
    assert_eq!(started.turn_started_at_ms, Some(ms(7)));
    assert_eq!(started.expires_at_ms, Some(ms(7) + 20_000));
    assert_eq!(balances(&started), vec![20_000, 20_000]);
    assert_eq!(clock(&runtime, 1), started, "every seat sees one clock");

    // Canonical evidence gains no wall-clock or time member: the tick already carries time.
    let value = canonical(&runtime);
    let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "admitted",
            "game",
            "last_admitted_player",
            "lifecycle",
            "version"
        ]
    );
    assert_eq!(value["version"], 1);
    assert!(value["game"].get("clock").is_some());
    assert_eq!(
        value["game"]["ruleset"]["time_control"],
        serde_json::to_value(rules.time_control).unwrap()
    );

    advance(&mut runtime, 30);
    assert_eq!(runtime.current_tick(), 37);
    assert_eq!(
        submit(&mut runtime, &one, 2, 0, Command::Pass {}).unwrap(),
        CommandOutcome::Applied
    );
    let next = clock(&runtime, 1);
    assert_eq!(next.turn_started_at_ms, Some(ms(37)));
    assert_eq!(next.expires_at_ms, Some(ms(37) + 20_000));
    assert_eq!(balances(&next), vec![20_000, 20_000], "per-turn resets");
    assert_eq!(view(&runtime, 1).public.phase, playing(2, 1));
}

#[test]
fn cumulative_commands_deduct_elapsed_tick_time_and_grant_the_increment_at_turn_start() {
    let rules = rules(cumulative(60_000, Some(5_000)), 10);
    let (mut runtime, [one, two]) = seated(&rules, GRACE);
    advance(&mut runtime, 10);
    start(&mut runtime, &one);
    let started = clock(&runtime, 1);
    assert_eq!(balances(&started), vec![65_000, 60_000]);
    assert_eq!(started.turn_started_at_ms, Some(500));
    assert_eq!(started.expires_at_ms, Some(65_500));

    advance(&mut runtime, 40);
    submit(&mut runtime, &one, 2, 0, Command::Pass {}).unwrap();
    let passed = clock(&runtime, 2);
    assert_eq!(balances(&passed), vec![63_000, 65_000]);
    assert_eq!(passed.turn_started_at_ms, Some(2_500));
    assert_eq!(passed.expires_at_ms, Some(67_500));

    advance(&mut runtime, 20);
    let rack = view(&runtime, 2).own_rack.tiles;
    let placements = rack
        .iter()
        .enumerate()
        .map(|(index, tile)| Placement {
            tile_id: tile.id,
            coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
            blank_as: None,
        })
        .collect();
    assert_eq!(
        submit(&mut runtime, &two, 1, 1, Command::Commit { placements }).unwrap(),
        CommandOutcome::Applied
    );
    let committed = view(&runtime, 1);
    assert_eq!(committed.public.players[1].score, 2);
    let after = committed.public.clock.unwrap();
    assert_eq!(balances(&after), vec![68_000, 64_000]);
    assert_eq!(after.turn_started_at_ms, Some(3_500));
    assert_eq!(after.expires_at_ms, Some(71_500));
}

#[test]
fn automatic_pass_fires_exactly_once_on_the_tick_that_reaches_expiry() {
    let rules = rules(per_turn(1_000, ExpiryConsequence::Pass), 10);
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    advance(&mut runtime, 3);
    start(&mut runtime, &one);
    assert_eq!(clock(&runtime, 1).expires_at_ms, Some(1_150));

    advance(&mut runtime, 19);
    assert_eq!(runtime.current_tick(), 22);
    assert!(passes(&runtime, 1).is_empty(), "no pass before expiry");
    assert_eq!(view(&runtime, 1).public.phase, playing(1, 0));

    advance(&mut runtime, 1);
    assert_eq!(
        passes(&runtime, 2),
        vec![(0, 1)],
        "one automatic pass at 1150 ms"
    );
    assert_eq!(view(&runtime, 1).public.phase, playing(2, 1));
    let next = clock(&runtime, 1);
    assert_eq!(next.turn_started_at_ms, Some(1_150));
    assert_eq!(next.expires_at_ms, Some(2_150));
    assert_eq!(balances(&next), vec![1_000, 1_000]);

    // The ended turn is gone: a late move for it changes nothing.
    let before = runtime.snapshot().unwrap();
    assert!(submit(&mut runtime, &one, 2, 0, Command::Pass {}).is_err());
    assert_eq!(runtime.snapshot().unwrap(), before);

    advance(&mut runtime, 19);
    assert_eq!(
        passes(&runtime, 1),
        vec![(0, 1)],
        "never a second pass for one expiry"
    );
    advance(&mut runtime, 1);
    assert_eq!(runtime.current_tick(), 43);
    assert_eq!(passes(&runtime, 1), vec![(0, 1), (1, 2)]);
}

#[test]
fn an_off_grid_expiry_passes_on_the_next_tick_and_the_next_turn_starts_there() {
    let rules = rules(per_turn(1_010, ExpiryConsequence::Pass), 10);
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    start(&mut runtime, &one);
    assert_eq!(clock(&runtime, 1).expires_at_ms, Some(1_010));
    advance(&mut runtime, 20);
    assert!(passes(&runtime, 1).is_empty(), "1000 ms is before 1010 ms");
    advance(&mut runtime, 1);
    assert_eq!(passes(&runtime, 1), vec![(0, 1)]);
    let next = clock(&runtime, 1);
    assert_eq!(next.turn_started_at_ms, Some(1_050));
    assert_eq!(next.expires_at_ms, Some(2_060));
    advance(&mut runtime, 20);
    assert_eq!(passes(&runtime, 1).len(), 1, "2050 ms is before 2060 ms");
    advance(&mut runtime, 1);
    assert_eq!(passes(&runtime, 1), vec![(0, 1), (1, 2)]);
    assert_eq!(clock(&runtime, 1).turn_started_at_ms, Some(2_100));
}

#[test]
fn cumulative_automatic_pass_exhausts_the_balance_and_the_grant_still_applies_at_zero() {
    let rules = rules(cumulative(1_000, Some(500)), 10);
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    start(&mut runtime, &one);
    assert_eq!(clock(&runtime, 1).expires_at_ms, Some(1_500));
    advance(&mut runtime, 29);
    assert!(passes(&runtime, 1).is_empty());
    advance(&mut runtime, 1);
    assert_eq!(passes(&runtime, 1), vec![(0, 1)]);
    let second = clock(&runtime, 1);
    assert_eq!(balances(&second), vec![0, 1_500]);
    assert_eq!(second.expires_at_ms, Some(3_000));
    advance(&mut runtime, 30);
    assert_eq!(passes(&runtime, 1), vec![(0, 1), (1, 2)]);
    let third = clock(&runtime, 1);
    assert_eq!(
        balances(&third),
        vec![500, 0],
        "Decision 3.8 grant on a zero balance"
    );
    assert_eq!(third.turn_started_at_ms, Some(3_000));
    assert_eq!(third.expires_at_ms, Some(3_500));
    advance(&mut runtime, 9);
    assert_eq!(passes(&runtime, 1).len(), 2);
    advance(&mut runtime, 1);
    assert_eq!(passes(&runtime, 1), vec![(0, 1), (1, 2), (2, 1)]);
}

#[test]
fn automatic_passes_can_finish_the_match_and_mark_lifecycle_completion() {
    let rules = rules(per_turn(1_000, ExpiryConsequence::Pass), 2);
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    start(&mut runtime, &one);
    advance(&mut runtime, 40);
    let finished = view(&runtime, 1);
    assert!(matches!(finished.public.phase, Phase::Finished { .. }));
    assert_eq!(passes(&runtime, 1), vec![(0, 1), (1, 2)]);
    assert_eq!(canonical(&runtime)["lifecycle"]["finished_at_tick"], 40);
    let frozen = finished.public.clock.unwrap();
    assert_eq!(frozen.turn_started_at_ms, None);
    assert_eq!(frozen.expires_at_ms, None);
    advance(&mut runtime, 100);
    assert_eq!(view(&runtime, 1).public.phase, finished.public.phase);
    assert_eq!(view(&runtime, 1).public.history, finished.public.history);
}

#[test]
fn automatic_pass_survives_canonical_replay_and_recovery_without_duplication() {
    let rules = rules(per_turn(1_000, ExpiryConsequence::Pass), 10);
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    start(&mut runtime, &one);
    advance(&mut runtime, 25);
    assert_eq!(passes(&runtime, 1), vec![(0, 1)]);
    let canonical = runtime.snapshot().unwrap();
    let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(fresh(&rules), &log).unwrap().final_snapshot,
        canonical,
        "canonical replay reproduces the automatic pass"
    );
    runtime.freeze_for_recovery();
    let image =
        RecoveryImage::decode(&runtime.recovery_image().unwrap().encode().unwrap()).unwrap();
    let mut recovered = MatchRuntime::restore_from_recovery(fresh(&rules), image).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), canonical);
    assert_eq!(recovered.current_tick(), 25);
    assert_eq!(passes(&recovered, 1), vec![(0, 1)]);
    advance(&mut recovered, 14);
    assert_eq!(
        passes(&recovered, 1),
        vec![(0, 1)],
        "restart applies no second pass"
    );
    advance(&mut recovered, 1);
    assert_eq!(passes(&recovered, 1), vec![(0, 1), (1, 2)]);

    // A recovery image taken on the expiry tick itself.
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    start(&mut runtime, &one);
    advance(&mut runtime, 20);
    assert_eq!(passes(&runtime, 1), vec![(0, 1)]);
    let at_expiry = runtime.snapshot().unwrap();
    runtime.freeze_for_recovery();
    let mut recovered =
        MatchRuntime::restore_from_recovery(fresh(&rules), runtime.recovery_image().unwrap())
            .unwrap();
    assert_eq!(recovered.snapshot().unwrap(), at_expiry);
    advance(&mut recovered, 1);
    assert_eq!(passes(&recovered, 1), vec![(0, 1)]);
    let next = clock(&recovered, 1);
    assert_eq!(next.turn_started_at_ms, Some(1_000));
    assert_eq!(next.expires_at_ms, Some(2_000));
}

#[test]
fn a_server_outage_freezes_remaining_time_across_recovery() {
    let rules = rules(cumulative(1_000, Some(500)), 10);
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    start(&mut runtime, &one);
    advance(&mut runtime, 10);
    let before = clock(&runtime, 1);
    assert_eq!(before.expires_at_ms, Some(1_500));
    runtime.freeze_for_recovery();
    let image = runtime.recovery_image().unwrap().encode().unwrap();
    drop(runtime);
    // Real downtime longer than the active player's whole remaining budget.
    std::thread::sleep(Duration::from_millis(1_600));
    let mut recovered =
        MatchRuntime::restore_from_recovery(fresh(&rules), RecoveryImage::decode(&image).unwrap())
            .unwrap();
    assert_eq!(recovered.current_tick(), 10);
    assert_eq!(clock(&recovered, 1), before, "no outage time is charged");
    assert!(
        passes(&recovered, 1).is_empty(),
        "no pass solely due to downtime"
    );
    advance(&mut recovered, 19);
    assert!(passes(&recovered, 1).is_empty());
    advance(&mut recovered, 1);
    assert_eq!(
        passes(&recovered, 1),
        vec![(0, 1)],
        "expiry after the saved remaining ticks"
    );
}

#[test]
fn a_disconnected_active_player_keeps_losing_time() {
    let rules = rules(per_turn(1_000, ExpiryConsequence::Pass), 10);
    let (mut runtime, [one, _two]) = seated(&rules, GRACE);
    start(&mut runtime, &one);
    advance(&mut runtime, 5);
    let before = clock(&runtime, 2);
    assert!(runtime.disconnect(1, one.connection_epoch));
    let observed = view(&runtime, 2);
    assert_eq!(observed.public.players[0].connected, Some(false));
    assert_eq!(
        observed.public.clock.unwrap(),
        before,
        "disconnect does not reset or extend"
    );
    advance(&mut runtime, 14);
    assert!(passes(&runtime, 2).is_empty());
    assert_eq!(clock(&runtime, 2), before);
    advance(&mut runtime, 1);
    assert_eq!(
        passes(&runtime, 2),
        vec![(0, 1)],
        "expiry while disconnected"
    );
    runtime
        .reconnect(ReconnectToken([1; 16]), ReconnectToken([3; 16]))
        .unwrap();
    assert_eq!(clock(&runtime, 1), clock(&runtime, 2));
    assert_eq!(view(&runtime, 1).public.history.len(), 1);

    // A started seat whose reconnect capability expired still times out on its turns.
    let (mut runtime, [one, _two]) = seated(&rules, 3);
    start(&mut runtime, &one);
    advance(&mut runtime, 5);
    assert!(runtime.disconnect(1, one.connection_epoch));
    advance(&mut runtime, 10);
    assert!(
        runtime.snapshot_for(1).is_err(),
        "the expired capability is retired"
    );
    advance(&mut runtime, 5);
    assert_eq!(passes(&runtime, 2), vec![(0, 1)]);
    advance(&mut runtime, 40);
    assert_eq!(passes(&runtime, 2), vec![(0, 1), (1, 2), (2, 1)]);
}

#[test]
fn forfeit_ticks_finish_once_and_replay_recovery_preserve_the_frozen_player() {
    for control in [
        per_turn(1_000, ExpiryConsequence::Forfeit),
        Some(TimeControl {
            clock: ClockModel::Cumulative {
                initial_ms: 1_000,
                increment_ms: None,
            },
            on_expiry: ExpiryConsequence::Forfeit,
        }),
    ] {
        let rules = rules(control, 10);
        let (mut runtime, [one, _two]) = seated(&rules, GRACE);
        start(&mut runtime, &one);
        let rack = view(&runtime, 1).own_rack;
        advance(&mut runtime, 19);
        assert_eq!(view(&runtime, 2).public.phase, playing(1, 0));
        advance(&mut runtime, 1);
        let finished = view(&runtime, 2);
        assert_eq!(
            finished.public.phase,
            Phase::Finished {
                winners: vec![PlayerId::new(2)]
            }
        );
        assert_eq!(finished.public.history.len(), 1);
        assert_eq!(view(&runtime, 1).own_rack, rack);
        assert_eq!(finished.public.players[0].score, 0);
        let saved = runtime.snapshot().unwrap();
        let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
        assert_eq!(
            verify_replay(fresh(&rules), &log).unwrap().final_snapshot,
            saved
        );
        runtime.freeze_for_recovery();
        let image =
            RecoveryImage::decode(&runtime.recovery_image().unwrap().encode().unwrap()).unwrap();
        let mut restored = MatchRuntime::restore_from_recovery(fresh(&rules), image).unwrap();
        assert_eq!(restored.snapshot().unwrap(), saved);
        advance(&mut restored, 100);
        let resumed = view(&restored, 2).public;
        assert_eq!(resumed.phase, finished.public.phase);
        assert_eq!(
            resumed.history, finished.public.history,
            "restart and late ticks cannot repeat a forfeit"
        );
        assert_eq!(resumed.clock, finished.public.clock);
        assert_eq!(
            resumed
                .players
                .iter()
                .map(|player| player.score)
                .collect::<Vec<_>>(),
            finished
                .public
                .players
                .iter()
                .map(|player| player.score)
                .collect::<Vec<_>>()
        );
        assert_eq!(view(&restored, 1).own_rack, rack);
        let stable = restored.snapshot().unwrap();
        assert!(submit(&mut restored, &one, 2, 0, Command::Pass {}).is_err());
        assert_eq!(restored.snapshot().unwrap(), stable);
    }
}

#[test]
fn multiplayer_forfeit_recovery_continues_rotation_and_disconnected_players_still_expire() {
    for count in 3..=4 {
        let mut rules = rules(per_turn(1_000, ExpiryConsequence::Forfeit), 100);
        rules.maximum_players = count;
        let mut runtime = MatchRuntime::new_with_replay_capture(fresh(&rules), GRACE);
        let leases: Vec<_> = (1..=count)
            .map(|id| runtime.admit(ReconnectToken([id; 16])).unwrap())
            .collect();
        start(&mut runtime, &leases[0]);
        let rack = view(&runtime, 1).own_rack;
        advance(&mut runtime, 20);
        assert_eq!(view(&runtime, 2).public.phase, playing(2, 1));
        let saved = runtime.snapshot().unwrap();
        let log = runtime.replay_log().unwrap().clone();
        assert_eq!(
            verify_replay(fresh(&rules), &log).unwrap().final_snapshot,
            saved
        );
        runtime.freeze_for_recovery();
        let mut restored =
            MatchRuntime::restore_from_recovery(fresh(&rules), runtime.recovery_image().unwrap())
                .unwrap();
        assert_eq!(restored.snapshot().unwrap(), saved);
        for id in 2..u64::from(count) {
            assert_eq!(view(&restored, 1).public.phase, playing(id, id - 1));
            // Recovery has no live connections. Disconnected survivors still expire.
            advance(&mut restored, 20);
            assert_eq!(view(&restored, 1).own_rack, rack);
        }
        assert_eq!(
            view(&restored, 1).public.phase,
            Phase::Finished {
                winners: vec![PlayerId::new(u64::from(count))]
            }
        );
        assert_eq!(
            view(&restored, 1).public.history.len(),
            usize::from(count - 1)
        );
    }
}

fn sha256(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Pinned on the pre-#114 baseline `1eb8b1b`: untimed evidence must not change by a byte.
#[test]
fn untimed_runtime_evidence_is_byte_identical_to_the_pre_clock_server() {
    let fresh = || {
        ScrabbleSimulation::new(GameId::new(1), english_fixture(), [7; 32])
            .unwrap()
            .with_lifecycle(1000)
    };
    let mut runtime = MatchRuntime::new_with_replay_capture(fresh(), 100);
    let one = runtime.admit(ReconnectToken([1; 16])).unwrap();
    let two = runtime.admit(ReconnectToken([2; 16])).unwrap();
    advance(&mut runtime, 3);
    submit(&mut runtime, &one, 1, 0, Command::Start {}).unwrap();
    advance(&mut runtime, 5);
    submit(&mut runtime, &one, 2, 0, Command::Pass {}).unwrap();
    advance(&mut runtime, 2);
    submit(&mut runtime, &two, 1, 1, Command::Pass {}).unwrap();
    advance(&mut runtime, 1);
    let public: Value = serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    assert!(public["public"].get("clock").is_none());
    assert!(
        public["public"]["configuration"]
            .get("time_control")
            .is_none()
    );
    let canonical = runtime.snapshot().unwrap();
    let evidence = [
        canonical.tick.to_string(),
        sha256(&canonical.payload),
        sha256(&runtime.snapshot_for(1).unwrap().payload),
        sha256(&runtime.snapshot_for(2).unwrap().payload),
        sha256(&runtime.replay_log().unwrap().encode().unwrap()),
    ];
    runtime.freeze_for_recovery();
    let image = runtime.recovery_image().unwrap().encode().unwrap();
    let mut evidence = evidence.to_vec();
    evidence.push(sha256(&image));
    assert_eq!(evidence, PINNED_UNTIMED_EVIDENCE, "{evidence:#?}");
    let recovered =
        MatchRuntime::restore_from_recovery(fresh(), RecoveryImage::decode(&image).unwrap())
            .unwrap();
    assert_eq!(recovered.snapshot().unwrap(), canonical);
}
const PINNED_UNTIMED_EVIDENCE: [&str; 6] = [
    "11",
    "9f5af2a2721416eda37e4f8b532c4d56ec68ddb8d70b609bf4e69412a58a1084",
    "8ffe43fca59a00e55f108ea11589fbd52b21e09297d1d170cee993a8012aaccd",
    "6ab23b8bb8a48ab13dc793644cfe22ff6c28e34a22c4d56b48d4fc5241b3e463",
    "a8887e4ce33cf78fc12afaceaa8d0255c5b6d7967f12ccf7403c374bd401619b",
    "da37f010e2ef401840e098c0090fc715c3e95ab7e1334763e35e3b5365e98f8f",
];
