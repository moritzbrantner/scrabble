use scrabble_game::{
    dictionary::WordList,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement},
    ruleset::{Premium, Ruleset, TileDefinition, TileFace, english_fixture},
    session::{CommandError, GameSession},
    state::{GameState, StateError},
};
use std::sync::Arc;

fn rules(players: u8, total: u16, limit: u16) -> Ruleset {
    let mut rules = english_fixture();
    rules.maximum_players = players;
    rules.rack_size = 2;
    rules.bingo_bonus = 0;
    rules.exchange_minimum_bag = 1;
    rules.scoreless_turn_limit = limit;
    rules.premiums.fill(Premium::Normal);
    rules.tiles = vec![TileDefinition {
        face: TileFace::Letter { letter: 'A' },
        count: total,
        value: 1,
    }];
    rules
}

fn started(rules: Ruleset, seed: u8) -> GameSession {
    let players = rules.maximum_players;
    let dictionary = Arc::new(WordList::from_text(rules.dictionary.clone(), "AA\n").unwrap());
    let mut session = GameSession::with_dictionary(
        GameState::new(GameId::new(1), rules, [seed; 32]).unwrap(),
        dictionary,
    );
    for player in 1..=players {
        session
            .add_player(PlayerId::new(u64::from(player)), format!("Player {player}"))
            .unwrap();
    }
    send(&mut session, 1, 1, 0, Command::Start {}).unwrap();
    session
}

fn send(
    session: &mut GameSession,
    player: u64,
    sequence: u32,
    turn: u64,
    command: Command,
) -> Result<(), CommandError> {
    session.apply(
        PlayerId::new(player),
        sequence,
        &CommandEnvelope {
            version: ProtocolVersion,
            game_id: GameId::new(1),
            player_id: PlayerId::new(player),
            sequence,
            expected_turn: TurnId::new(turn),
            command,
        },
    )
}

fn opening(session: &GameSession, player: u64) -> Vec<Placement> {
    session
        .state()
        .rack(PlayerId::new(player))
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, tile)| Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
            blank_as: matches!(tile.face(), TileFace::Blank {}).then_some('A'),
        })
        .collect()
}

fn scores(session: &GameSession) -> Vec<i32> {
    session
        .public_snapshot()
        .players
        .iter()
        .map(|player| player.score)
        .collect()
}

#[test]
fn going_out_transfers_every_opponents_rack_value_once_and_freezes_all_commands() {
    for players in 2..=4 {
        let mut session = started(rules(players, u16::from(players) * 2, 6), 7);
        let placements = opening(&session, 1);
        send(
            &mut session,
            1,
            2,
            0,
            Command::Preview {
                placements: placements.clone(),
            },
        )
        .unwrap();
        let ending = Command::Commit { placements };
        send(&mut session, 1, 3, 0, ending.clone()).unwrap();
        assert_eq!(
            session.public_snapshot().phase,
            Phase::Finished {
                winners: vec![PlayerId::new(1)]
            }
        );
        let expected: Vec<_> = (0..players)
            .map(|index| {
                if index == 0 {
                    i32::from(players) * 2
                } else {
                    -2
                }
            })
            .collect();
        assert_eq!(scores(&session), expected);
        assert!(session.public_snapshot().preview.is_none());
        assert_eq!(session.public_snapshot().remaining_tiles, 0);
        session.state().verify_tile_conservation().unwrap();
        let before = session.state().canonical_bytes().unwrap();
        assert_eq!(
            send(&mut session, 1, 3, 0, ending.clone()),
            Err(CommandError::StaleSequence)
        );
        for command in [
            ending,
            Command::Pass {},
            Command::Exchange { tile_ids: vec![] },
            Command::Preview { placements: vec![] },
            Command::Start {},
            Command::SetName {
                display_name: "Changed".into(),
            },
        ] {
            assert_eq!(
                send(&mut session, 1, 4, 0, command),
                Err(CommandError::State(StateError::WrongPhase))
            );
            assert_eq!(session.state().canonical_bytes().unwrap(), before);
        }
    }
}

#[test]
fn scoreless_limit_deducts_racks_without_a_transfer_and_keeps_every_tied_winner() {
    let mut session = started(rules(3, 10, 3), 3);
    let racks: Vec<_> = (1..=3)
        .map(|id| session.player_snapshot(PlayerId::new(id)).unwrap().own_rack)
        .collect();
    for turn in 0..3 {
        send(
            &mut session,
            turn + 1,
            if turn == 0 { 2 } else { 1 },
            turn,
            Command::Pass {},
        )
        .unwrap();
        if turn < 2 {
            assert!(matches!(
                session.public_snapshot().phase,
                Phase::Playing { .. }
            ));
        }
    }
    assert_eq!(scores(&session), vec![-2, -2, -2]);
    assert_eq!(
        session.public_snapshot().phase,
        Phase::Finished {
            winners: (1..=3).map(PlayerId::new).collect(),
        }
    );
    assert_eq!(session.public_snapshot().remaining_tiles, 4);
    for (index, rack) in racks.iter().enumerate() {
        assert_eq!(
            &session
                .player_snapshot(PlayerId::new(index as u64 + 1))
                .unwrap()
                .own_rack,
            rack
        );
    }
    session.state().verify_tile_conservation().unwrap();
}

#[test]
fn zero_score_commit_exchange_and_pass_share_the_consecutive_counter() {
    let mut configured = rules(2, 10, 3);
    configured.tiles = vec![
        TileDefinition {
            face: TileFace::Letter { letter: 'A' },
            count: 2,
            value: 1,
        },
        TileDefinition {
            face: TileFace::Blank {},
            count: 8,
            value: 0,
        },
    ];
    let mut session = (0..=255)
        .map(|seed| started(configured.clone(), seed))
        .find(|session| {
            session
                .state()
                .rack(PlayerId::new(1))
                .unwrap()
                .iter()
                .all(|tile| matches!(tile.face(), TileFace::Blank {}))
        })
        .unwrap();
    let placements = opening(&session, 1);
    send(&mut session, 1, 2, 0, Command::Commit { placements }).unwrap();
    assert_eq!(scores(&session), vec![0, 0]);
    let tile = session.state().rack(PlayerId::new(2)).unwrap()[0].id();
    send(
        &mut session,
        2,
        1,
        1,
        Command::Exchange {
            tile_ids: vec![tile],
        },
    )
    .unwrap();
    assert!(matches!(
        session.public_snapshot().phase,
        Phase::Playing { .. }
    ));
    let rack_values: Vec<i32> = (1..=2)
        .map(|id| {
            session
                .state()
                .rack(PlayerId::new(id))
                .unwrap()
                .iter()
                .map(|tile| i32::from(tile.value()))
                .sum()
        })
        .collect();
    send(&mut session, 1, 3, 2, Command::Pass {}).unwrap();
    assert!(matches!(
        session.public_snapshot().phase,
        Phase::Finished { .. }
    ));
    assert_eq!(
        scores(&session),
        rack_values
            .into_iter()
            .map(|value| -value)
            .collect::<Vec<_>>()
    );
    assert_eq!(session.public_snapshot().board.len(), 2);
    session.state().verify_tile_conservation().unwrap();
}

#[test]
fn zero_point_go_out_takes_precedence_when_both_end_conditions_are_met() {
    let mut configured = rules(2, 4, 1);
    configured.tiles = vec![
        TileDefinition {
            face: TileFace::Letter { letter: 'A' },
            count: 2,
            value: 1,
        },
        TileDefinition {
            face: TileFace::Blank {},
            count: 2,
            value: 0,
        },
    ];
    let mut session = (0..=255)
        .map(|seed| started(configured.clone(), seed))
        .find(|session| {
            session
                .state()
                .rack(PlayerId::new(1))
                .unwrap()
                .iter()
                .all(|tile| matches!(tile.face(), TileFace::Blank {}))
        })
        .unwrap();
    let placements = opening(&session, 1);
    send(&mut session, 1, 2, 0, Command::Commit { placements }).unwrap();
    assert_eq!(scores(&session), vec![2, -2]);
    assert_eq!(
        session.public_snapshot().phase,
        Phase::Finished {
            winners: vec![PlayerId::new(1)]
        }
    );
    assert!(session.state().rack(PlayerId::new(1)).unwrap().is_empty());
    session.state().verify_tile_conservation().unwrap();
}

#[test]
fn positive_commit_resets_the_counter_and_failed_commands_do_not_count() {
    let mut session = started(rules(2, 12, 3), 7);
    send(&mut session, 1, 2, 0, Command::Pass {}).unwrap();
    send(&mut session, 2, 1, 1, Command::Pass {}).unwrap();
    let before = session.state().canonical_bytes().unwrap();
    assert!(
        send(
            &mut session,
            1,
            3,
            2,
            Command::Commit { placements: vec![] }
        )
        .is_err()
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), before);
    let placements = opening(&session, 1);
    send(&mut session, 1, 3, 2, Command::Commit { placements }).unwrap();
    assert_eq!(scores(&session), vec![2, 0]);
    for (player, sequence, turn) in [(2, 2, 3), (1, 4, 4)] {
        send(&mut session, player, sequence, turn, Command::Pass {}).unwrap();
        assert!(matches!(
            session.public_snapshot().phase,
            Phase::Playing { .. }
        ));
    }
    send(&mut session, 2, 3, 5, Command::Pass {}).unwrap();
    assert_eq!(scores(&session), vec![0, -2]);
    assert_eq!(
        session.public_snapshot().phase,
        Phase::Finished {
            winners: vec![PlayerId::new(1)]
        }
    );
}
