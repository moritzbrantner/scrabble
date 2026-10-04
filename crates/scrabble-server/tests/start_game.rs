use game_server::{GameSimulation, MatchRuntime, ReconnectToken, ReplayLog, verify_replay};
use scrabble_game::{
    identity::{GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, PlayerSnapshot},
};
use scrabble_server::{
    config::ServerConfig,
    factory::{MatchFactory, creation_time, game_id},
    simulation::ScrabbleSimulation,
};
use std::collections::{BTreeMap, BTreeSet};

fn factory(directory: &std::path::Path) -> MatchFactory {
    let config = ServerConfig::from_values(&BTreeMap::from([
        ("SCRABBLE_MATCH_IDS".into(), String::new()),
        ("SCRABBLE_TEST_SEED".into(), "07".repeat(32)),
        (
            "SCRABBLE_SEED_FILE".into(),
            directory.join("seed").to_str().unwrap().into(),
        ),
        (
            "SCRABBLE_RECOVERY_DIR".into(),
            directory.join("recovery").to_str().unwrap().into(),
        ),
    ]))
    .unwrap();
    MatchFactory::new(&config).unwrap()
}
fn command(game: GameId, actor: u32, sequence: u32, command: Command) -> Vec<u8> {
    serde_json::to_vec(&CommandEnvelope {
        version: ProtocolVersion,
        game_id: game,
        player_id: PlayerId::new(u64::from(actor)),
        sequence,
        expected_turn: TurnId::new(0),
        command,
    })
    .unwrap()
}
fn projected(simulation: &ScrabbleSimulation, actor: u32) -> PlayerSnapshot {
    serde_json::from_slice(&simulation.snapshot_for(actor).unwrap().payload).unwrap()
}

#[test]
fn board_proof_is_bound_to_version_match_and_persisted_factory() {
    let directory = tempfile::tempdir().unwrap();
    let first = factory(directory.path());
    let nonce = "01".repeat(16);
    let legacy = first.creation_id(&nonce, 1000).unwrap();
    let board = first.board_creation_id(&nonce, 1000).unwrap();
    assert_ne!(legacy, board);
    assert_eq!(creation_time(&legacy), Some(1000));
    assert_eq!(creation_time(&board), Some(1000));
    assert!(first.board_authority(&legacy).is_err());
    let authority = first.board_authority(&board).unwrap();
    assert!(authority.verify(&nonce));
    for invalid in ["02".repeat(16), "01".repeat(15), "AB".repeat(16)] {
        assert!(!authority.verify(&invalid));
    }
    let later = first.board_creation_id(&nonce, 1001).unwrap();
    assert_ne!(board, later);
    let another = first.board_creation_id(&"02".repeat(16), 1000).unwrap();
    assert!(!first.board_authority(&another).unwrap().verify(&nonce));
    let restarted = factory(directory.path());
    assert_eq!(restarted.creation_id(&nonce, 1000).unwrap(), legacy);
    assert_eq!(restarted.board_creation_id(&nonce, 1000).unwrap(), board);
    assert!(restarted.board_authority(&board).unwrap().verify(&nonce));
    let canonical: serde_json::Value =
        serde_json::from_slice(&first.create(&legacy).unwrap().snapshot().unwrap().payload)
            .unwrap();
    assert_eq!(canonical["version"], 1);
    assert!(canonical.get("board").is_none());
}

#[test]
fn board_has_no_seat_or_rack_and_four_phones_receive_one_deterministic_deal() {
    let directory = tempfile::tempdir().unwrap();
    let factory = factory(directory.path());
    let nonce = "01".repeat(16);
    let id = factory.board_creation_id(&nonce, 1000).unwrap();
    let game = game_id(&id);
    let mut simulation = factory.create(&id).unwrap();
    assert_eq!(simulation.max_players(), 5);
    simulation.add_player(1).unwrap();
    let before = simulation.snapshot().unwrap();
    assert!(
        simulation
            .apply_command(
                1,
                1,
                &command(
                    game,
                    1,
                    1,
                    Command::ClaimBoard {
                        request_id: "02".repeat(16)
                    }
                )
            )
            .is_err()
    );
    assert_eq!(simulation.snapshot().unwrap(), before);
    simulation
        .apply_command(
            1,
            1,
            &command(
                game,
                1,
                1,
                Command::ClaimBoard {
                    request_id: nonce.clone(),
                },
            ),
        )
        .unwrap();
    assert_eq!(
        projected(&simulation, 1).public.host.unwrap().id,
        Some(PlayerId::new(1))
    );
    for actor in 2..=5 {
        simulation.add_player(actor).unwrap();
        assert!(
            projected(&simulation, actor)
                .public
                .players
                .iter()
                .all(|player| player.id.get() != u64::from(actor))
        );
        simulation
            .apply_command(
                actor,
                1,
                &command(
                    game,
                    actor,
                    1,
                    Command::SetName {
                        display_name: format!("Phone {actor}"),
                    },
                ),
            )
            .unwrap();
        if actor == 2 {
            let before = simulation.snapshot().unwrap();
            assert!(
                simulation
                    .apply_command(1, 2, &command(game, 1, 2, Command::Start {}))
                    .is_err()
            );
            assert_eq!(simulation.snapshot().unwrap(), before);
        }
    }
    let before = simulation.snapshot().unwrap();
    assert!(simulation.add_player(6).is_err());
    assert!(
        simulation
            .apply_command(2, 2, &command(game, 2, 2, Command::Start {}))
            .is_err()
    );
    assert!(
        simulation
            .apply_command(
                1,
                2,
                &command(
                    game,
                    1,
                    2,
                    Command::SetName {
                        display_name: "Board".into()
                    }
                )
            )
            .is_err()
    );
    assert!(
        simulation
            .apply_command(
                2,
                2,
                &command(game, 2, 2, Command::ClaimBoard { request_id: nonce })
            )
            .is_err()
    );
    assert_eq!(simulation.snapshot().unwrap(), before);
    simulation
        .apply_command(1, 2, &command(game, 1, 2, Command::Start {}))
        .unwrap();
    let board = projected(&simulation, 1);
    assert!(board.own_rack.tiles.is_empty());
    assert_eq!(
        board
            .public
            .players
            .iter()
            .map(|player| player.id.get())
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    assert_eq!(board.public.remaining_tiles, 72);
    assert_eq!(
        board.public.phase,
        Phase::Playing {
            active_player: PlayerId::new(2),
            turn: TurnId::new(0)
        }
    );
    let mut dealt = BTreeSet::new();
    for actor in 2..=5 {
        let phone = projected(&simulation, actor);
        assert_eq!(phone.public, board.public);
        assert_eq!(phone.own_rack.tiles.len(), 7);
        assert!(
            phone
                .own_rack
                .tiles
                .iter()
                .all(|tile| dealt.insert(tile.id))
        );
    }
    let canonical: serde_json::Value =
        serde_json::from_slice(&simulation.snapshot().unwrap().payload).unwrap();
    let mut all_ids: BTreeSet<String> = dealt.into_iter().map(|id| id.get().to_string()).collect();
    for tile in canonical["game"]["bag"].as_array().unwrap() {
        assert!(all_ids.insert(tile["id"].as_str().unwrap().into()));
    }
    assert_eq!(all_ids.len(), 100);
    let before = simulation.snapshot().unwrap();
    assert!(
        simulation
            .apply_command(1, 3, &command(game, 1, 3, Command::Start {}))
            .is_err()
    );
    assert!(
        simulation
            .apply_command(
                2,
                2,
                &command(
                    game,
                    2,
                    2,
                    Command::SetName {
                        display_name: "Late".into()
                    }
                )
            )
            .is_err()
    );
    assert!(simulation.add_player(6).is_err());
    assert_eq!(simulation.snapshot().unwrap(), before);
}

#[test]
fn separate_board_authority_and_deal_survive_runtime_replay_and_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let factory = factory(directory.path());
    let nonce = "01".repeat(16);
    let id = factory.board_creation_id(&nonce, 1000).unwrap();
    let game = game_id(&id);
    let make = || factory.create(&id).unwrap();
    let mut runtime = MatchRuntime::new_with_replay_capture(make(), 100);
    let board = runtime.admit(ReconnectToken([1; 16])).unwrap();
    runtime
        .submit_command(
            board.player_id,
            board.connection_epoch,
            1,
            &command(
                game,
                board.player_id,
                1,
                Command::ClaimBoard { request_id: nonce },
            ),
        )
        .unwrap();
    runtime.advance_tick().unwrap();
    for token in 2..=5 {
        let phone = runtime.admit(ReconnectToken([token; 16])).unwrap();
        runtime
            .submit_command(
                phone.player_id,
                phone.connection_epoch,
                1,
                &command(
                    game,
                    phone.player_id,
                    1,
                    Command::SetName {
                        display_name: format!("Phone {token}"),
                    },
                ),
            )
            .unwrap();
        runtime.advance_tick().unwrap();
    }
    runtime
        .submit_command(
            board.player_id,
            board.connection_epoch,
            2,
            &command(game, board.player_id, 2, Command::Start {}),
        )
        .unwrap();
    runtime.advance_tick().unwrap();
    let expected = runtime.snapshot().unwrap();
    let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(make(), &log).unwrap().final_snapshot,
        expected
    );
    runtime.freeze_for_recovery();
    let restored =
        MatchRuntime::restore_from_recovery(make(), runtime.recovery_image().unwrap()).unwrap();
    assert_eq!(restored.snapshot().unwrap(), expected);
    for actor in 1..=5 {
        let mut before: PlayerSnapshot =
            serde_json::from_slice(&runtime.snapshot_for(actor).unwrap().payload).unwrap();
        let mut after: PlayerSnapshot =
            serde_json::from_slice(&restored.snapshot_for(actor).unwrap().payload).unwrap();
        assert!(
            before
                .public
                .players
                .iter()
                .all(|player| player.connected == Some(true))
        );
        assert!(
            after
                .public
                .players
                .iter()
                .all(|player| player.connected == Some(false))
        );
        for snapshot in [&mut before, &mut after] {
            for player in &mut snapshot.public.players {
                player.connected = None;
            }
        }
        assert_eq!(before, after);
    }
    let board: PlayerSnapshot =
        serde_json::from_slice(&restored.snapshot_for(1).unwrap().payload).unwrap();
    assert!(board.own_rack.tiles.is_empty());
}

#[test]
fn separate_board_commands_validate_identity_sequence_turn_and_names_before_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let factory = factory(directory.path());
    let nonce = "01".repeat(16);
    let id = factory.board_creation_id(&nonce, 1000).unwrap();
    let game = game_id(&id);
    let mut simulation = factory.create(&id).unwrap();
    simulation.add_player(1).unwrap();
    simulation.add_player(2).unwrap();
    for kind in [
        Command::ClaimBoard {
            request_id: nonce.clone(),
        },
        Command::SetName {
            display_name: "Ada".into(),
        },
    ] {
        let actor = if matches!(kind, Command::ClaimBoard { .. }) {
            1
        } else {
            2
        };
        let original: CommandEnvelope =
            serde_json::from_slice(&command(game, actor, 1, kind)).unwrap();
        let before = simulation.snapshot().unwrap();
        let mut wrong_identity = original.clone();
        wrong_identity.player_id = PlayerId::new(99);
        let mut wrong_game = original.clone();
        wrong_game.game_id = GameId::new(0);
        let mut wrong_sequence = original.clone();
        wrong_sequence.sequence = 2;
        let mut wrong_turn = original.clone();
        wrong_turn.expected_turn = TurnId::new(1);
        for invalid in [wrong_identity, wrong_game, wrong_sequence, wrong_turn] {
            assert!(
                simulation
                    .apply_command(actor, 1, &serde_json::to_vec(&invalid).unwrap())
                    .is_err()
            );
            assert_eq!(simulation.snapshot().unwrap(), before);
        }
        simulation
            .apply_command(actor, 1, &serde_json::to_vec(&original).unwrap())
            .unwrap();
        let before = simulation.snapshot().unwrap();
        assert!(
            simulation
                .apply_command(actor, 1, &serde_json::to_vec(&original).unwrap())
                .is_err()
        );
        assert_eq!(simulation.snapshot().unwrap(), before);
    }
    simulation.add_player(3).unwrap();
    for name in [String::new(), " ".into(), "A".repeat(33), "A\nB".into()] {
        let before = simulation.snapshot().unwrap();
        assert!(
            simulation
                .apply_command(
                    3,
                    1,
                    &command(game, 3, 1, Command::SetName { display_name: name })
                )
                .is_err()
        );
        assert_eq!(simulation.snapshot().unwrap(), before);
    }
    assert!(simulation.try_remove_player(3).unwrap());
    assert!(simulation.try_remove_player(1).unwrap());
    simulation.add_player(4).unwrap();
    simulation
        .apply_command(
            4,
            1,
            &command(game, 4, 1, Command::ClaimBoard { request_id: nonce }),
        )
        .unwrap();
    assert_eq!(
        projected(&simulation, 4).public.host.unwrap().id,
        Some(PlayerId::new(4))
    );
    assert!(projected(&simulation, 4).own_rack.tiles.is_empty());
}
