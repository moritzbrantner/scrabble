//! Independent acceptance contract for the authoritative match clock (#112).
//! Specification: docs/specs/match-clock.md. Uses only the public engine API.
//!
//! Every instant is injected authoritative milliseconds; the engine never reads a clock.
//! Fixture: two players, racks of two `A` tiles worth 1, a normal board and the word `AA`,
//! so the opening commit at (7, 7)–(7, 8) always scores 2.
use scrabble_game::{
    commit::{self, CommitError},
    dictionary::WordList,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TileId, TurnId},
    protocol::{
        Command, CommandEnvelope, Phase, Placement, PlayerClock, PublicClock, PublicSnapshot,
        TurnAction,
    },
    ruleset::{
        ClockModel, ExpiryConsequence, MAX_CLOCK_MS, Premium, Ruleset, TileDefinition, TileFace,
        TimeControl, english_fixture,
    },
    session::{CommandError, GameSession},
    state::{ClockError, GameState, StateError},
};
use std::sync::Arc;

const P1: u64 = 1;
const P2: u64 = 2;

fn base_rules(scoreless_turn_limit: u16) -> Ruleset {
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
    rules
}
fn cumulative(
    initial_ms: u64,
    increment_ms: Option<u64>,
    on_expiry: ExpiryConsequence,
) -> TimeControl {
    TimeControl {
        clock: ClockModel::Cumulative {
            initial_ms,
            increment_ms,
        },
        on_expiry,
    }
}
fn per_turn(allowance_ms: u64, on_expiry: ExpiryConsequence) -> TimeControl {
    TimeControl {
        clock: ClockModel::PerTurn { allowance_ms },
        on_expiry,
    }
}
fn timed(control: TimeControl, scoreless_turn_limit: u16) -> Ruleset {
    let mut rules = base_rules(scoreless_turn_limit);
    rules.time_control = Some(control);
    rules
}
/// Lobby session with both players seated, not started.
fn lobby(rules: Ruleset, seed: u8) -> GameSession {
    let dictionary = Arc::new(WordList::from_text(rules.dictionary.clone(), "AA\n").unwrap());
    let mut session = GameSession::with_dictionary(
        GameState::new(GameId::new(1), rules, [seed; 32]).unwrap(),
        dictionary,
    );
    session.add_player(PlayerId::new(P1), "Ada".into()).unwrap();
    session.add_player(PlayerId::new(P2), "Lin".into()).unwrap();
    session
}
fn envelope(player: u64, sequence: u32, turn: u64, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(player),
        sequence,
        expected_turn: TurnId::new(turn),
        command,
    }
}
fn at(
    session: &mut GameSession,
    player: u64,
    sequence: u32,
    turn: u64,
    command: Command,
    now_ms: u64,
) -> Result<(), CommandError> {
    session.apply_at(
        PlayerId::new(player),
        sequence,
        &envelope(player, sequence, turn, command),
        now_ms,
    )
}
fn untimed_send(
    session: &mut GameSession,
    player: u64,
    sequence: u32,
    turn: u64,
    command: Command,
) -> Result<(), CommandError> {
    session.apply(
        PlayerId::new(player),
        sequence,
        &envelope(player, sequence, turn, command),
    )
}
fn timeout(session: &mut GameSession, turn: u64, now_ms: u64) -> Result<(), CommandError> {
    session.timeout_at(TurnId::new(turn), now_ms)
}
fn clock_error(error: ClockError) -> CommandError {
    CommandError::State(StateError::Clock(error))
}
fn opening(session: &GameSession, player: u64) -> Vec<Placement> {
    placements_at(session, player, 7, 7)
}
fn placements_at(session: &GameSession, player: u64, row: u8, column: u8) -> Vec<Placement> {
    session
        .state()
        .rack(PlayerId::new(player))
        .unwrap()
        .iter()
        .take(2)
        .enumerate()
        .map(|(index, tile)| Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(row, column + index as u8).unwrap(),
            blank_as: None,
        })
        .collect()
}
fn first_tile(session: &GameSession, player: u64) -> TileId {
    session.state().rack(PlayerId::new(player)).unwrap()[0].id()
}
/// The public clock: per-player remaining ms (seat order), turn start and expiry.
fn clock(session: &GameSession) -> (Vec<(u64, u64)>, Option<u64>, Option<u64>) {
    let clock = session
        .public_snapshot()
        .clock
        .expect("timed matches publish a clock");
    assert_eq!(clock.expires_at_ms, session.state().clock_expires_at());
    (
        clock
            .players
            .iter()
            .map(|entry| (entry.player_id.get(), entry.remaining_ms))
            .collect(),
        clock.turn_started_at_ms,
        clock.expires_at_ms,
    )
}
fn playing(session: &GameSession, player: u64, turn: u64) {
    assert_eq!(
        *session.state().phase(),
        Phase::Playing {
            active_player: PlayerId::new(player),
            turn: TurnId::new(turn),
        }
    );
}
fn scores(session: &GameSession) -> Vec<i32> {
    session
        .public_snapshot()
        .players
        .iter()
        .map(|player| player.score)
        .collect()
}
/// Canonical bytes plus the public snapshot: everything a rejection must leave untouched.
fn evidence(session: &GameSession) -> (Vec<u8>, PublicSnapshot) {
    (
        session.state().canonical_bytes().unwrap(),
        session.public_snapshot(),
    )
}

#[test]
fn time_control_configuration_is_bounded_and_rejects_cumulative_pass_without_increment() {
    use ExpiryConsequence::{Forfeit, Pass};
    assert_eq!(MAX_CLOCK_MS, 86_400_000);
    let valid = [
        cumulative(1, Some(1), Pass),
        cumulative(MAX_CLOCK_MS, Some(MAX_CLOCK_MS), Pass),
        cumulative(60_000, Some(5_000), Forfeit),
        // Decision 3.8: cumulative + forfeit needs no increment.
        cumulative(60_000, None, Forfeit),
        per_turn(1, Pass),
        per_turn(MAX_CLOCK_MS, Forfeit),
    ];
    for control in valid {
        let rules = timed(control, 6);
        assert_eq!(rules.validate(), Ok(()), "{control:?} must be valid");
        assert!(GameState::new(GameId::new(1), rules, [0; 32]).is_ok());
    }
    let invalid = [
        cumulative(0, Some(5_000), Pass),
        cumulative(MAX_CLOCK_MS + 1, Some(5_000), Pass),
        cumulative(60_000, Some(0), Pass),
        cumulative(60_000, Some(0), Forfeit),
        cumulative(60_000, Some(MAX_CLOCK_MS + 1), Pass),
        // Decision 3.8: cumulative + automatic pass requires a strictly positive increment.
        cumulative(60_000, None, Pass),
        cumulative(MAX_CLOCK_MS, None, Pass),
        per_turn(0, Pass),
        per_turn(0, Forfeit),
        per_turn(MAX_CLOCK_MS + 1, Pass),
    ];
    for control in invalid {
        let rules = timed(control, 6);
        assert!(rules.validate().is_err(), "{control:?} must be rejected");
        assert!(matches!(
            GameState::new(GameId::new(1), rules, [0; 32]),
            Err(StateError::InvalidRuleset(_))
        ));
    }
    // Untimed stays valid and is the default of every fixture (Decision 3.11).
    assert_eq!(english_fixture().time_control, None);
    assert_eq!(base_rules(6).validate(), Ok(()));
}

#[test]
fn time_control_json_shape_round_trips_and_rejects_unknown_fields() {
    for (control, json) in [
        (
            cumulative(60_000, Some(5_000), ExpiryConsequence::Pass),
            r#"{"clock":{"kind":"cumulative","initial_ms":60000,"increment_ms":5000},"on_expiry":"pass"}"#,
        ),
        (
            cumulative(60_000, None, ExpiryConsequence::Forfeit),
            r#"{"clock":{"kind":"cumulative","initial_ms":60000},"on_expiry":"forfeit"}"#,
        ),
        (
            per_turn(20_000, ExpiryConsequence::Forfeit),
            r#"{"clock":{"kind":"per_turn","allowance_ms":20000},"on_expiry":"forfeit"}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&control).unwrap(), json);
        assert_eq!(serde_json::from_str::<TimeControl>(json).unwrap(), control);
        let rules = timed(control, 6);
        let text = serde_json::to_string(&rules).unwrap();
        assert!(text.contains(&format!(r#""time_control":{json}"#)));
        assert_eq!(serde_json::from_str::<Ruleset>(&text).unwrap(), rules);
    }
    let rules = timed(cumulative(60_000, Some(5_000), ExpiryConsequence::Pass), 6);
    let value = serde_json::to_value(&rules).unwrap();
    let mutations: [fn(&mut serde_json::Value); 4] = [
        |v| v["time_control"]["clock"]["kind"] = "hourglass".into(),
        |v| v["time_control"]["on_expiry"] = "warn".into(),
        |v| v["time_control"]["pause"] = true.into(),
        |v| v["time_control"]["clock"]["allowance_ms"] = 1.into(),
    ];
    for mutate in mutations {
        let mut changed = value.clone();
        mutate(&mut changed);
        assert!(serde_json::from_value::<Ruleset>(changed).is_err());
    }
}

#[test]
fn cumulative_clock_carries_balance_and_grants_increment_at_every_turn_start() {
    let mut session = lobby(
        timed(cumulative(60_000, Some(5_000), ExpiryConsequence::Pass), 6),
        3,
    );
    // Lobby: starting balances, no running turn.
    assert_eq!(
        clock(&session),
        (vec![(P1, 60_000), (P2, 60_000)], None, None)
    );
    assert_eq!(session.state().clock_expires_at(), None);

    at(&mut session, P1, 1, 0, Command::Start {}, 1_000_000).unwrap();
    // The first turn also receives the increment.
    assert_eq!(
        clock(&session),
        (
            vec![(P1, 65_000), (P2, 60_000)],
            Some(1_000_000),
            Some(1_065_000)
        )
    );

    // Commit after 10 s: deducted; P2's turn begins at the commit instant with its grant.
    let placements = opening(&session, P1);
    at(
        &mut session,
        P1,
        2,
        0,
        Command::Commit { placements },
        1_010_000,
    )
    .unwrap();
    playing(&session, P2, 1);
    assert_eq!(scores(&session), [2, 0]);
    assert_eq!(
        clock(&session),
        (
            vec![(P1, 55_000), (P2, 65_000)],
            Some(1_010_000),
            Some(1_075_000)
        )
    );

    // Exchange after 30 s.
    let tile = first_tile(&session, P2);
    at(
        &mut session,
        P2,
        1,
        1,
        Command::Exchange {
            tile_ids: vec![tile],
        },
        1_040_000,
    )
    .unwrap();
    assert_eq!(
        clock(&session),
        (
            vec![(P1, 60_000), (P2, 35_000)],
            Some(1_040_000),
            Some(1_100_000)
        )
    );

    // Pass after 1 s.
    at(&mut session, P1, 3, 2, Command::Pass {}, 1_041_000).unwrap();
    assert_eq!(
        clock(&session),
        (
            vec![(P1, 59_000), (P2, 40_000)],
            Some(1_041_000),
            Some(1_081_000)
        )
    );

    // P2 runs out; the automatic pass leaves a zero balance.
    timeout(&mut session, 3, 1_081_000).unwrap();
    assert_eq!(
        clock(&session),
        (
            vec![(P1, 64_000), (P2, 0)],
            Some(1_081_000),
            Some(1_145_000)
        )
    );

    // The increment is granted even on a zero balance (Decision 3.8).
    at(&mut session, P1, 4, 4, Command::Pass {}, 1_090_000).unwrap();
    playing(&session, P2, 5);
    assert_eq!(
        clock(&session),
        (
            vec![(P1, 55_000), (P2, 5_000)],
            Some(1_090_000),
            Some(1_095_000)
        )
    );
}

#[test]
fn per_turn_clock_starts_fresh_and_discards_unused_time() {
    let mut session = lobby(timed(per_turn(20_000, ExpiryConsequence::Pass), 6), 3);
    assert_eq!(
        clock(&session),
        (vec![(P1, 20_000), (P2, 20_000)], None, None)
    );
    at(&mut session, P1, 1, 0, Command::Start {}, 500).unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 20_000), (P2, 20_000)], Some(500), Some(20_500))
    );
    let placements = opening(&session, P1);
    at(
        &mut session,
        P1,
        2,
        0,
        Command::Commit { placements },
        19_000,
    )
    .unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 20_000), (P2, 20_000)], Some(19_000), Some(39_000))
    );
    // P2 uses only 1 s; the unused 19 s are discarded rather than carried.
    at(&mut session, P2, 1, 1, Command::Pass {}, 20_000).unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 20_000), (P2, 20_000)], Some(20_000), Some(40_000))
    );
    // One millisecond before expiry is still in time.
    at(&mut session, P1, 3, 2, Command::Pass {}, 39_999).unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 20_000), (P2, 20_000)], Some(39_999), Some(59_999))
    );
    // Per-turn expiry: automatic pass, then the next turn is fresh again.
    assert_eq!(
        at(&mut session, P2, 2, 3, Command::Pass {}, 59_999),
        Err(clock_error(ClockError::Expired))
    );
    timeout(&mut session, 3, 60_500).unwrap();
    playing(&session, P1, 4);
    assert_eq!(
        clock(&session),
        (vec![(P1, 20_000), (P2, 20_000)], Some(60_500), Some(80_500))
    );
}

#[test]
fn rejected_commands_leave_clock_turn_start_and_sequence_unchanged() {
    let mut session = lobby(
        timed(cumulative(60_000, Some(5_000), ExpiryConsequence::Pass), 6),
        3,
    );
    at(&mut session, P1, 1, 0, Command::Start {}, 1_000).unwrap();
    let before = evidence(&session);
    let off_center = placements_at(&session, P1, 0, 0);
    // Invalid placement (the opening must cover the center).
    assert!(matches!(
        at(
            &mut session,
            P1,
            2,
            0,
            Command::Commit {
                placements: off_center
            },
            5_000
        ),
        Err(CommandError::Commit(_))
    ));
    assert_eq!(evidence(&session), before);
    // An instant before the active turn start.
    assert_eq!(
        at(&mut session, P1, 2, 0, Command::Pass {}, 999),
        Err(clock_error(ClockError::TimeRegression))
    );
    assert_eq!(evidence(&session), before);
    // Not the active player, and a stale turn.
    assert_eq!(
        at(&mut session, P2, 1, 0, Command::Pass {}, 5_000),
        Err(CommandError::NotActive)
    );
    assert_eq!(
        at(&mut session, P1, 2, 1, Command::Pass {}, 5_000),
        Err(CommandError::StaleTurn)
    );
    // An exchange without tiles.
    assert_eq!(
        at(
            &mut session,
            P1,
            2,
            0,
            Command::Exchange { tile_ids: vec![] },
            5_000
        ),
        Err(CommandError::State(StateError::EmptyExchange))
    );
    assert_eq!(evidence(&session), before);
    assert_eq!(session.state().clock_expires_at(), Some(66_000));

    // Sequence 2 was never consumed; elapsed time still counts from the original start.
    let placements = opening(&session, P1);
    at(
        &mut session,
        P1,
        2,
        0,
        Command::Commit { placements },
        11_000,
    )
    .unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 55_000), (P2, 65_000)], Some(11_000), Some(76_000))
    );
}

#[test]
fn expiry_applies_automatic_pass_exactly_once_and_rejects_later_moves() {
    let mut session = lobby(
        timed(cumulative(60_000, Some(5_000), ExpiryConsequence::Pass), 6),
        3,
    );
    at(&mut session, P1, 1, 0, Command::Start {}, 0).unwrap();
    assert_eq!(session.state().clock_expires_at(), Some(65_000));
    let placements = opening(&session, P1);
    at(
        &mut session,
        P1,
        2,
        0,
        Command::Preview {
            placements: placements.clone(),
        },
        1_000,
    )
    .unwrap();
    assert!(session.public_snapshot().preview.is_some());

    // At and after the expiry instant every turn command is too late, deterministically.
    let before = evidence(&session);
    for (command, now) in [
        (
            Command::Commit {
                placements: placements.clone(),
            },
            65_000,
        ),
        (
            Command::Preview {
                placements: placements.clone(),
            },
            65_000,
        ),
        (Command::Pass {}, 70_000),
        (
            Command::Exchange {
                tile_ids: vec![placements[0].tile_id],
            },
            90_000,
        ),
    ] {
        assert_eq!(
            at(&mut session, P1, 3, 0, command, now),
            Err(clock_error(ClockError::Expired))
        );
        assert_eq!(evidence(&session), before);
    }
    // A timeout before expiry, or for another turn, changes nothing.
    assert_eq!(
        timeout(&mut session, 0, 64_999),
        Err(clock_error(ClockError::NotExpired))
    );
    assert_eq!(
        timeout(&mut session, 1, 65_000),
        Err(CommandError::StaleTurn)
    );
    assert_eq!(evidence(&session), before);

    // Exactly at expiry: one automatic pass through ordinary turn completion.
    timeout(&mut session, 0, 65_000).unwrap();
    playing(&session, P2, 1);
    let snapshot = session.public_snapshot();
    assert_eq!(snapshot.preview, None);
    assert_eq!(snapshot.history.len(), 1);
    let turn = &snapshot.history[0];
    assert_eq!(turn.turn, TurnId::new(0));
    assert_eq!(turn.player_id, PlayerId::new(P1));
    assert_eq!(turn.action, TurnAction::Pass {});
    assert!(
        turn.scores
            .iter()
            .all(|change| change.delta == 0 && change.score == 0)
    );
    assert_eq!(
        clock(&session),
        (vec![(P1, 0), (P2, 65_000)], Some(65_000), Some(130_000))
    );

    // Duplicate or late timeouts for the ended turn are rejected without effect.
    let after = evidence(&session);
    for now in [65_000, 65_001, 200_000] {
        assert_eq!(timeout(&mut session, 0, now), Err(CommandError::StaleTurn));
    }
    assert_eq!(evidence(&session), after);

    // A late but valid timeout: the next turn starts at the instant it is applied.
    timeout(&mut session, 1, 140_000).unwrap();
    playing(&session, P1, 2);
    assert_eq!(session.public_snapshot().history.len(), 2);
    assert_eq!(
        clock(&session),
        (vec![(P1, 5_000), (P2, 0)], Some(140_000), Some(145_000))
    );

    // The timeouts consumed no player sequence: P1's rejected sequence 3 is still free.
    let placements = opening(&session, P1);
    at(
        &mut session,
        P1,
        3,
        2,
        Command::Commit { placements },
        141_000,
    )
    .unwrap();
    assert_eq!(scores(&session), [2, 0]);
    assert_eq!(
        clock(&session),
        (vec![(P1, 4_000), (P2, 5_000)], Some(141_000), Some(146_000))
    );
}

#[test]
fn automatic_passes_count_toward_the_scoreless_ending_and_freeze_the_finished_clock() {
    let mut session = lobby(
        timed(cumulative(10_000, Some(1_000), ExpiryConsequence::Pass), 3),
        3,
    );
    at(&mut session, P1, 1, 0, Command::Start {}, 0).unwrap();
    timeout(&mut session, 0, 11_000).unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 0), (P2, 11_000)], Some(11_000), Some(22_000))
    );
    at(&mut session, P2, 1, 1, Command::Pass {}, 12_000).unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 1_000), (P2, 10_000)], Some(12_000), Some(13_000))
    );
    // The third consecutive scoreless turn is a (late) automatic pass and ends the match.
    timeout(&mut session, 2, 20_000).unwrap();
    assert_eq!(
        *session.state().phase(),
        Phase::Finished {
            winners: vec![PlayerId::new(P1), PlayerId::new(P2)]
        }
    );
    assert_eq!(scores(&session), [-2, -2]);
    let snapshot = session.public_snapshot();
    let last = snapshot.history.last().unwrap();
    assert_eq!(
        (last.turn, last.player_id, &last.action),
        (TurnId::new(2), PlayerId::new(P1), &TurnAction::Pass {})
    );
    assert!(last.scores.iter().all(|change| change.delta == -2));
    assert_eq!(clock(&session), (vec![(P1, 0), (P2, 10_000)], None, None));
    assert_eq!(session.state().clock_expires_at(), None);

    let finished = evidence(&session);
    for turn in [2, 3] {
        assert_eq!(
            timeout(&mut session, turn, 30_000),
            Err(CommandError::State(StateError::WrongPhase))
        );
    }
    assert_eq!(
        at(&mut session, P2, 2, 3, Command::Pass {}, 30_000),
        Err(CommandError::State(StateError::WrongPhase))
    );
    assert_eq!(evidence(&session), finished);
}

#[test]
fn timed_matches_reject_untimed_entry_points() {
    let rules = timed(per_turn(20_000, ExpiryConsequence::Pass), 6);
    let mut session = lobby(rules.clone(), 3);
    // Lobby naming involves no clock.
    untimed_send(
        &mut session,
        P2,
        1,
        0,
        Command::SetName {
            display_name: "Lin B".into(),
        },
    )
    .unwrap();
    let lobby_evidence = evidence(&session);
    assert_eq!(
        untimed_send(&mut session, P1, 1, 0, Command::Start {}),
        Err(clock_error(ClockError::InstantRequired))
    );
    assert_eq!(
        session.start_by_authorized_board(
            PlayerId::new(P1),
            1,
            &envelope(P1, 1, 0, Command::Start {})
        ),
        Err(clock_error(ClockError::InstantRequired))
    );
    let mut direct = session.state().clone();
    assert_eq!(
        direct.deal_initial_racks(),
        Err(StateError::Clock(ClockError::InstantRequired))
    );
    assert_eq!(evidence(&session), lobby_evidence);

    session
        .start_by_authorized_board_at(
            PlayerId::new(P1),
            1,
            &envelope(P1, 1, 0, Command::Start {}),
            7_000,
        )
        .unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 20_000), (P2, 20_000)], Some(7_000), Some(27_000))
    );
    // Starting through the storage boundary is equivalent.
    let mut direct = session_state_before_start(&rules);
    direct.deal_initial_racks_at(7_000).unwrap();
    assert_eq!(direct.clock_expires_at(), Some(27_000));

    let playing_evidence = evidence(&session);
    let placements = opening(&session, P1);
    let tile = first_tile(&session, P1);
    for command in [
        Command::Preview {
            placements: placements.clone(),
        },
        Command::Commit {
            placements: placements.clone(),
        },
        Command::Pass {},
        Command::Exchange {
            tile_ids: vec![tile],
        },
    ] {
        assert_eq!(
            untimed_send(&mut session, P1, 2, 0, command),
            Err(clock_error(ClockError::InstantRequired))
        );
    }
    let mut direct = session.state().clone();
    assert_eq!(
        commit::apply(&mut direct, PlayerId::new(P1), &placements).err(),
        Some(CommitError::State(StateError::Clock(
            ClockError::InstantRequired
        )))
    );
    assert_eq!(direct.canonical_bytes().unwrap(), playing_evidence.0);
    assert_eq!(evidence(&session), playing_evidence);
    // Sequence 2 is still available to the timed entry point.
    at(&mut session, P1, 2, 0, Command::Pass {}, 8_000).unwrap();
}

fn session_state_before_start(rules: &Ruleset) -> GameState {
    let mut state = GameState::new(GameId::new(1), rules.clone(), [3; 32]).unwrap();
    state.add_player(PlayerId::new(P1), "Ada".into()).unwrap();
    state.add_player(PlayerId::new(P2), "Lin".into()).unwrap();
    state
}

#[test]
fn forfeit_is_configurable_but_never_applied_until_the_owner_decides() {
    // Cumulative + forfeit without increment: balances carry, and no grant is added.
    let mut session = lobby(
        timed(cumulative(30_000, None, ExpiryConsequence::Forfeit), 6),
        3,
    );
    at(&mut session, P1, 1, 0, Command::Start {}, 0).unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 30_000), (P2, 30_000)], Some(0), Some(30_000))
    );
    at(&mut session, P1, 2, 0, Command::Pass {}, 10_000).unwrap();
    at(&mut session, P2, 1, 1, Command::Pass {}, 15_000).unwrap();
    assert_eq!(
        clock(&session),
        (vec![(P1, 20_000), (P2, 25_000)], Some(15_000), Some(35_000))
    );
    let before = evidence(&session);
    assert_eq!(
        timeout(&mut session, 2, 34_999),
        Err(clock_error(ClockError::NotExpired))
    );
    // Provisional: no forfeit outcome is applied, and moves stay expired.
    for now in [35_000, 90_000] {
        assert_eq!(
            timeout(&mut session, 2, now),
            Err(clock_error(ClockError::ForfeitUnavailable))
        );
        assert_eq!(
            at(&mut session, P1, 3, 2, Command::Pass {}, now),
            Err(clock_error(ClockError::Expired))
        );
    }
    assert_eq!(evidence(&session), before);
    playing(&session, P1, 2);

    let mut session = lobby(timed(per_turn(20_000, ExpiryConsequence::Forfeit), 6), 3);
    at(&mut session, P1, 1, 0, Command::Start {}, 0).unwrap();
    let before = evidence(&session);
    assert_eq!(
        timeout(&mut session, 0, 20_000),
        Err(clock_error(ClockError::ForfeitUnavailable))
    );
    assert_eq!(evidence(&session), before);
}

enum Step {
    Apply(u64, u32, u64, Command, u64),
    Timeout(u64, u64),
}
fn run(session: &mut GameSession, steps: &[Step]) {
    for step in steps {
        match step {
            Step::Apply(player, sequence, turn, command, now) => {
                at(session, *player, *sequence, *turn, command.clone(), *now).unwrap();
            }
            Step::Timeout(turn, now) => timeout(session, *turn, *now).unwrap(),
        }
    }
}

#[test]
fn timed_match_serializes_reconstructs_and_replays_canonically() {
    let control = cumulative(60_000, Some(5_000), ExpiryConsequence::Pass);
    let rules = timed(control, 6);
    let mut original = lobby(rules.clone(), 3);
    let mut steps = vec![Step::Apply(P1, 1, 0, Command::Start {}, 1_000)];
    run(&mut original, &steps);
    let placements = opening(&original, P1);
    let mut tail = vec![
        Step::Apply(P1, 2, 0, Command::Commit { placements }, 4_000),
        Step::Timeout(1, 70_000),
        Step::Apply(P1, 3, 2, Command::Pass {}, 71_000),
    ];
    run(&mut original, &tail);
    steps.append(&mut tail);
    let tile = first_tile(&original, P2);
    let mut tail = vec![Step::Apply(
        P2,
        1,
        3,
        Command::Exchange {
            tile_ids: vec![tile],
        },
        72_500,
    )];
    run(&mut original, &tail);
    steps.append(&mut tail);
    playing(&original, P1, 4);
    assert_eq!(
        clock(&original),
        (vec![(P1, 71_000), (P2, 3_500)], Some(72_500), Some(143_500))
    );

    // Canonical evidence carries the configuration and the private clock state, version 3.
    let canonical = original.state().canonical_bytes().unwrap();
    let evidence_json: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
    assert_eq!(evidence_json["version"], 3);
    assert!(evidence_json.get("clock").is_some());
    assert_eq!(
        serde_json::from_value::<TimeControl>(evidence_json["ruleset"]["time_control"].clone())
            .unwrap(),
        control
    );
    let reconstructed: Ruleset = serde_json::from_value(evidence_json["ruleset"].clone()).unwrap();
    assert_eq!(reconstructed, rules);

    // Replaying the same calls and instants reproduces bytes and snapshots exactly.
    let mut replay = lobby(reconstructed, 3);
    run(&mut replay, &steps);
    assert_eq!(replay.state().canonical_bytes().unwrap(), canonical);
    assert_eq!(replay.public_snapshot(), original.public_snapshot());
    replay.state().verify_tile_conservation().unwrap();

    // Public snapshot: model, remaining times and expiry only; no private data.
    let snapshot = original.public_snapshot();
    assert_eq!(snapshot.configuration.time_control, Some(control));
    assert_eq!(
        snapshot.clock,
        Some(PublicClock {
            players: vec![
                PlayerClock {
                    player_id: PlayerId::new(P1),
                    remaining_ms: 71_000
                },
                PlayerClock {
                    player_id: PlayerId::new(P2),
                    remaining_ms: 3_500
                },
            ],
            turn_started_at_ms: Some(72_500),
            expires_at_ms: Some(143_500),
        })
    );
    let value = serde_json::to_value(&snapshot).unwrap();
    let mut keys: Vec<_> = value["clock"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, ["expires_at_ms", "players", "turn_started_at_ms"]);
    for entry in value["clock"]["players"].as_array().unwrap() {
        let mut keys: Vec<_> = entry.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["player_id", "remaining_ms"]);
    }
    assert_eq!(
        serde_json::from_value::<PublicSnapshot>(value.clone()).unwrap(),
        snapshot
    );
    let mut unknown = value["clock"].clone();
    unknown["paused"] = true.into();
    assert!(serde_json::from_value::<PublicClock>(unknown).is_err());
    assert_eq!(
        original.player_snapshot(PlayerId::new(P1)).unwrap().public,
        snapshot
    );
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
fn pinned(bytes: &[u8]) -> (usize, u64) {
    (bytes.len(), fnv1a(bytes))
}

/// Lengths and FNV-1a hashes were captured on the pre-#112 baseline ebf2095.
/// Untimed matches through either entry point, with any (even non-monotonic) instants,
/// stay byte-identical and expose no clock.
#[test]
fn untimed_matches_stay_byte_identical_and_ignore_instants() {
    for timed_entry in [false, true] {
        let send = |session: &mut GameSession,
                    player: u64,
                    sequence: u32,
                    turn: u64,
                    command: Command,
                    now: u64| {
            if timed_entry {
                at(session, player, sequence, turn, command, now)
            } else {
                untimed_send(session, player, sequence, turn, command)
            }
        };
        // English fixture: start, pass, exchange.
        let mut session =
            GameSession::new(GameState::new(GameId::new(1), english_fixture(), [7; 32]).unwrap());
        session.add_player(PlayerId::new(P1), "Ada".into()).unwrap();
        session.add_player(PlayerId::new(P2), "Lin".into()).unwrap();
        send(&mut session, P1, 1, 0, Command::Start {}, 50_000).unwrap();
        send(&mut session, P1, 2, 0, Command::Pass {}, 10).unwrap();
        let tile = first_tile(&session, P2);
        send(
            &mut session,
            P2,
            1,
            1,
            Command::Exchange {
                tile_ids: vec![tile],
            },
            u64::MAX,
        )
        .unwrap();
        assert_eq!(session.state().clock_expires_at(), None);
        assert_eq!(session.public_snapshot().clock, None);
        assert_eq!(
            pinned(&session.state().canonical_bytes().unwrap()),
            (11970, 0x0bd4_61d5_62ee_fcc1)
        );
        assert_eq!(
            pinned(&serde_json::to_vec(&session.public_snapshot()).unwrap()),
            (5016, 0x61e4_7b4e_2f6f_4e15)
        );
        assert_eq!(
            pinned(
                &serde_json::to_vec(&session.player_snapshot(PlayerId::new(P1)).unwrap()).unwrap()
            ),
            (5415, 0x2b49_6271_c836_fd46)
        );

        // Commit fixture.
        let rules = base_rules(6);
        let text = serde_json::to_vec(&rules).unwrap();
        assert!(
            !String::from_utf8(text.clone())
                .unwrap()
                .contains("time_control")
        );
        assert_eq!(pinned(&text), (2367, 0x04fe_9b5f_ab93_3cd6));
        let mut session = lobby(rules, 3);
        send(&mut session, P1, 1, 0, Command::Start {}, 9).unwrap();
        let placements = opening(&session, P1);
        send(&mut session, P1, 2, 0, Command::Commit { placements }, 0).unwrap();
        let canonical = session.state().canonical_bytes().unwrap();
        assert!(
            !String::from_utf8(canonical.clone())
                .unwrap()
                .contains("clock")
        );
        assert_eq!(pinned(&canonical), (6392, 0x6b4f_eeb9_6db7_8c31));
        let snapshot = serde_json::to_vec(&session.public_snapshot()).unwrap();
        assert!(
            !String::from_utf8(snapshot.clone())
                .unwrap()
                .contains("clock")
        );
        assert_eq!(pinned(&snapshot), (3109, 0x12f3_0616_873d_3d29));

        // No timeout exists for an untimed match.
        let before = evidence(&session);
        assert_eq!(
            timeout(&mut session, 1, u64::MAX),
            Err(clock_error(ClockError::Untimed))
        );
        assert_eq!(evidence(&session), before);
    }
}
