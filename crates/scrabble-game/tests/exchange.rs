use scrabble_game::{
    identity::{GameId, PlayerId, ProtocolVersion, TileId, TurnId},
    protocol::{Command, CommandEnvelope, Phase},
    ruleset::{TileDefinition, TileFace, english_fixture},
    session::{CommandError, GameSession},
    state::{GameState, StateError},
};
use serde_json::Value;

fn command(player: u64, sequence: u32, turn: u64, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(player),
        sequence,
        expected_turn: TurnId::new(turn),
        command,
    }
}
fn apply(session: &mut GameSession, command: &CommandEnvelope) -> Result<(), CommandError> {
    session.apply(command.player_id, command.sequence, command)
}
fn started(bag: u16, minimum: u16, seed: u8) -> GameSession {
    let mut rules = english_fixture();
    rules.rack_size = 2;
    rules.maximum_players = 2;
    rules.exchange_minimum_bag = minimum;
    rules.tiles = vec![
        TileDefinition {
            face: TileFace::Letter { letter: 'A' },
            count: bag + 2,
            value: 1,
        },
        TileDefinition {
            face: TileFace::Blank {},
            count: 2,
            value: 0,
        },
    ];
    let mut session = GameSession::new(GameState::new(GameId::new(1), rules, [seed; 32]).unwrap());
    session.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    session.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    apply(&mut session, &command(1, 1, 0, Command::Start {})).unwrap();
    session
}
fn canonical(session: &GameSession) -> Value {
    serde_json::from_slice(&session.state().canonical_bytes().unwrap()).unwrap()
}

#[test]
fn near_empty_exchange_draws_exactly_selected_identities_and_preserves_every_other_tile() {
    for bag in [2, 3] {
        for count in [1, 2] {
            for seed in 0..16 {
                let mut session = started(bag, 2, seed);
                let before = canonical(&session);
                let rack = session.state().rack(PlayerId::new(1)).unwrap().to_vec();
                let selected: Vec<_> = rack.iter().take(count).map(|tile| tile.id()).collect();
                let expected: Vec<_> = before["bag"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rev()
                    .take(count)
                    .cloned()
                    .collect();
                apply(
                    &mut session,
                    &command(
                        1,
                        2,
                        0,
                        Command::Exchange {
                            tile_ids: selected.clone(),
                        },
                    ),
                )
                .unwrap();
                let after = canonical(&session);
                assert_eq!(after["board"], before["board"]);
                assert_eq!(after["players"][1], before["players"][1]);
                assert_eq!(after["players"][0]["score"], before["players"][0]["score"]);
                assert_eq!(after["bag"].as_array().unwrap().len(), usize::from(bag));
                let resulting = after["players"][0]["rack"].as_array().unwrap();
                assert_eq!(&resulting[2 - count..], expected);
                assert_eq!(
                    &resulting[..2 - count],
                    &before["players"][0]["rack"].as_array().unwrap()[count..]
                );
                for id in &selected {
                    let serialized_id = id.get().to_string();
                    assert!(
                        !session
                            .state()
                            .rack(PlayerId::new(1))
                            .unwrap()
                            .iter()
                            .any(|tile| tile.id() == *id)
                    );
                    assert_eq!(
                        after["bag"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|tile| tile["id"] == serialized_id)
                            .count(),
                        1
                    );
                }
                assert_eq!(
                    session.state().phase(),
                    &Phase::Playing {
                        active_player: PlayerId::new(2),
                        turn: TurnId::new(1)
                    }
                );
                session.state().verify_tile_conservation().unwrap();
            }
        }
    }
}

#[test]
fn exchange_rejections_preserve_state_and_leave_sequence_available() {
    let mut session = started(2, 2, 7);
    let owned = session.state().rack(PlayerId::new(1)).unwrap()[0].id();
    let other = session.state().rack(PlayerId::new(2)).unwrap()[0].id();
    let before = canonical(&session);
    for (ids, error) in [
        (vec![], StateError::EmptyExchange),
        (vec![owned, owned], StateError::DuplicateTile),
        (vec![other], StateError::TileNotOwned),
        (vec![TileId::new(999)], StateError::TileNotOwned),
    ] {
        assert_eq!(
            apply(
                &mut session,
                &command(1, 2, 0, Command::Exchange { tile_ids: ids })
            ),
            Err(CommandError::State(error))
        );
        assert_eq!(canonical(&session), before);
    }
    for (player, turn, error) in [
        (2, 0, CommandError::NotActive),
        (1, 99, CommandError::StaleTurn),
    ] {
        assert_eq!(
            apply(
                &mut session,
                &command(
                    player,
                    2,
                    turn,
                    Command::Exchange {
                        tile_ids: vec![if player == 2 { other } else { owned }]
                    }
                )
            ),
            Err(error)
        );
        assert_eq!(canonical(&session), before);
    }
    let exchange = command(
        1,
        2,
        0,
        Command::Exchange {
            tile_ids: vec![owned],
        },
    );
    apply(&mut session, &exchange).unwrap();
    let after = canonical(&session);
    assert_eq!(
        apply(&mut session, &exchange),
        Err(CommandError::StaleSequence)
    );
    assert_eq!(
        apply(&mut session, &command(1, 3, 0, exchange.command)),
        Err(CommandError::StaleTurn)
    );
    assert_eq!(canonical(&session), after);
    for (bag, minimum, count) in [(1, 2, 1), (0, 0, 1), (1, 0, 2)] {
        let mut session = started(bag, minimum, 7);
        let before = canonical(&session);
        let ids = session
            .state()
            .rack(PlayerId::new(1))
            .unwrap()
            .iter()
            .take(count)
            .map(|tile| tile.id())
            .collect();
        assert_eq!(
            apply(
                &mut session,
                &command(1, 2, 0, Command::Exchange { tile_ids: ids })
            ),
            Err(CommandError::State(StateError::ExchangeUnavailable))
        );
        assert_eq!(canonical(&session), before);
    }
}

#[test]
fn blank_exchange_and_multiple_turns_replay_independently_of_selection_order() {
    let seed = (0..=255)
        .find(|seed| {
            started(3, 2, *seed)
                .state()
                .rack(PlayerId::new(1))
                .unwrap()
                .iter()
                .any(|tile| tile.face() == (TileFace::Blank {}))
        })
        .unwrap();
    let mut session = started(3, 2, seed);
    let mut replay = started(3, 2, seed);
    for (player, seq, turn) in [(1, 2, 0), (2, 1, 1), (1, 3, 2)] {
        let ids: Vec<_> = session
            .state()
            .rack(PlayerId::new(player))
            .unwrap()
            .iter()
            .map(|tile| tile.id())
            .collect();
        apply(
            &mut session,
            &command(
                player,
                seq,
                turn,
                Command::Exchange {
                    tile_ids: ids.clone(),
                },
            ),
        )
        .unwrap();
        apply(
            &mut replay,
            &command(
                player,
                seq,
                turn,
                Command::Exchange {
                    tile_ids: ids.into_iter().rev().collect(),
                },
            ),
        )
        .unwrap();
        assert_eq!(canonical(&session), canonical(&replay));
        session.state().verify_tile_conservation().unwrap();
    }
}
