use game_server::{CommandOutcome, MatchRuntime, ReconnectToken, verify_replay};
use scrabble_game::{
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{
        Command, CommandEnvelope, MAX_HISTORY_TURNS, Phase, Placement, PlayerSnapshot, TurnAction,
    },
    ruleset::{TileFace, english_fixture},
};
use scrabble_server::simulation::ScrabbleSimulation;

fn fresh(seed: [u8; 32], scoreless: u16) -> ScrabbleSimulation {
    let mut rules = english_fixture();
    rules.scoreless_turn_limit = scoreless;
    ScrabbleSimulation::new(GameId::new(1), rules, seed).unwrap()
}
fn read(runtime: &MatchRuntime<ScrabbleSimulation>, actor: u32) -> PlayerSnapshot {
    serde_json::from_slice(&runtime.snapshot_for(actor).unwrap().payload).unwrap()
}
fn send(
    runtime: &mut MatchRuntime<ScrabbleSimulation>,
    actor: u32,
    epoch: u32,
    seq: u32,
    command: Command,
) {
    let Phase::Playing { turn, .. } = read(runtime, actor).public.phase else {
        panic!("not playing")
    };
    let envelope = CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(u64::from(actor)),
        sequence: seq,
        expected_turn: turn,
        command,
    };
    assert_eq!(
        runtime
            .submit_command(actor, epoch, seq, &serde_json::to_vec(&envelope).unwrap())
            .unwrap(),
        CommandOutcome::Applied
    );
}
fn started(seed: [u8; 32], scoreless: u16) -> MatchRuntime<ScrabbleSimulation> {
    let mut runtime = MatchRuntime::new_with_replay_capture(fresh(seed, scoreless), 100);
    runtime.admit(ReconnectToken([1; 16])).unwrap();
    runtime.admit(ReconnectToken([2; 16])).unwrap();
    let start = CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(1),
        sequence: 1,
        expected_turn: TurnId::new(0),
        command: Command::Start {},
    };
    runtime
        .submit_command(1, 1, 1, &serde_json::to_vec(&start).unwrap())
        .unwrap();
    runtime
}

#[test]
fn blanks_words_exchange_pass_completion_and_reconnect_reproduce_one_authoritative_history() {
    let (seed, mut runtime, placements, c) = (0_u16..4096)
        .find_map(|number| {
            let mut seed = [7; 32];
            seed[..2].copy_from_slice(&number.to_be_bytes());
            let runtime = started(seed, 6);
            let first = read(&runtime, 1);
            let second = read(&runtime, 2);
            let a = first
                .own_rack
                .tiles
                .iter()
                .find(|tile| tile.face == TileFace::Letter { letter: 'A' })?;
            let blank = first
                .own_rack
                .tiles
                .iter()
                .find(|tile| tile.face == (TileFace::Blank {}))?;
            let c = second
                .own_rack
                .tiles
                .iter()
                .find(|tile| tile.face == TileFace::Letter { letter: 'C' })?;
            let placements = vec![
                Placement {
                    tile_id: a.id,
                    coordinate: Coordinate::new(7, 7).unwrap(),
                    blank_as: None,
                },
                Placement {
                    tile_id: blank.id,
                    coordinate: Coordinate::new(7, 8).unwrap(),
                    blank_as: Some('T'),
                },
            ];
            Some((seed, runtime, placements, c.id))
        })
        .expect("deterministic A/blank and C racks");
    send(
        &mut runtime,
        1,
        1,
        2,
        Command::Preview {
            placements: placements.clone(),
        },
    );
    assert!(read(&runtime, 1).public.history.is_empty());
    let before = runtime.snapshot().unwrap();
    let bad = CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(1),
        sequence: 3,
        expected_turn: TurnId::new(0),
        command: Command::Commit { placements: vec![] },
    };
    assert!(
        runtime
            .submit_command(1, 1, 3, &serde_json::to_vec(&bad).unwrap())
            .is_err()
    );
    assert_eq!(runtime.snapshot().unwrap(), before);
    send(&mut runtime, 1, 1, 3, Command::Commit { placements });
    let opening = read(&runtime, 1).public;
    assert_eq!(
        opening.history[0].action,
        TurnAction::Commit {
            words: vec!["AT".into()],
            move_score: 2,
            blank_count: 1
        }
    );
    assert_eq!(opening.history[0].scores[0].delta, 2);
    assert!(
        opening
            .board
            .iter()
            .any(|tile| tile.is_blank && tile.letter == 'T')
    );
    assert!(runtime.disconnect(2, 1));
    let resumed = runtime
        .reconnect(ReconnectToken([2; 16]), ReconnectToken([22; 16]))
        .unwrap();
    assert_eq!(resumed.connection_epoch, 2);
    assert_eq!(read(&runtime, 2).public.history, opening.history);
    send(
        &mut runtime,
        2,
        2,
        1,
        Command::Commit {
            placements: vec![Placement {
                tile_id: c,
                coordinate: Coordinate::new(7, 6).unwrap(),
                blank_as: None,
            }],
        },
    );
    let normal = read(&runtime, 1).public;
    assert_eq!(
        normal.history[1].action,
        TurnAction::Commit {
            words: vec!["CAT".into()],
            move_score: 4,
            blank_count: 0
        }
    );
    let exchanged = read(&runtime, 1).own_rack.tiles[0].id;
    send(
        &mut runtime,
        1,
        1,
        4,
        Command::Exchange {
            tile_ids: vec![exchanged],
        },
    );
    assert_eq!(
        serde_json::to_value(&read(&runtime, 2).public.history[2].action).unwrap(),
        serde_json::json!({"kind":"exchange", "tile_count":1})
    );
    let mut sequences = [4, 1];
    for _ in 0..5 {
        let Phase::Playing { active_player, .. } = read(&runtime, 1).public.phase else {
            panic!("early completion")
        };
        let actor = active_player.get() as u32;
        let index = actor as usize - 1;
        sequences[index] += 1;
        send(
            &mut runtime,
            actor,
            if actor == 1 { 1 } else { 2 },
            sequences[index],
            Command::Pass {},
        );
    }
    let finished = read(&runtime, 1).public;
    assert!(matches!(finished.phase, Phase::Finished { .. }));
    assert_eq!(finished.history.len(), 8);
    for (index, entry) in finished.history.iter().enumerate() {
        assert_eq!(entry.turn.get(), index as u64);
        assert_eq!(entry.player_id.get(), 1 + index as u64 % 2);
        if index >= 3 {
            assert_eq!(entry.action, TurnAction::Pass {});
        }
    }
    let mut accumulated = [0_i64; 2];
    for entry in &finished.history {
        for (index, change) in entry.scores.iter().enumerate() {
            accumulated[index] += change.delta;
            assert_eq!(accumulated[index], i64::from(change.score));
        }
    }
    assert_eq!(
        accumulated,
        [
            i64::from(finished.players[0].score),
            i64::from(finished.players[1].score)
        ]
    );
    assert!(
        finished
            .history
            .last()
            .unwrap()
            .scores
            .iter()
            .all(|score| score.delta < 0)
    );
    assert_eq!(read(&runtime, 2).public.history, finished.history);
    runtime.freeze_for_recovery();
    let canonical = runtime.snapshot().unwrap();
    let image = runtime.recovery_image().unwrap();
    let verified = verify_replay(fresh(seed, 6), &image.replay).unwrap();
    assert_eq!(verified.final_snapshot, canonical);
    let restored = MatchRuntime::restore_from_recovery(fresh(seed, 6), image).unwrap();
    assert_eq!(restored.snapshot().unwrap(), canonical);
    assert_eq!(read(&restored, 1).public.history, finished.history);
}

#[test]
fn bounded_history_keeps_absolute_turn_order_and_full_replay_evidence() {
    let seed = [7; 32];
    let mut runtime = started(seed, 100);
    let mut sequences = [1, 0];
    for turn in 0..(MAX_HISTORY_TURNS + 9) {
        let actor = 1 + turn as u32 % 2;
        let index = actor as usize - 1;
        sequences[index] += 1;
        send(&mut runtime, actor, 1, sequences[index], Command::Pass {});
    }
    let public = read(&runtime, 1).public;
    assert_eq!(public.history.len(), MAX_HISTORY_TURNS);
    assert_eq!(public.history.first().unwrap().turn.get(), 9);
    assert_eq!(
        public.history.last().unwrap().turn.get(),
        (MAX_HISTORY_TURNS + 8) as u64
    );
    let verified = verify_replay(fresh(seed, 100), runtime.replay_log().unwrap()).unwrap();
    assert_eq!(verified.final_snapshot, runtime.snapshot().unwrap());
    assert!(verified.records_verified > MAX_HISTORY_TURNS);
}

#[test]
fn largest_supported_public_history_stays_inside_native_snapshot_byte_limit() {
    use scrabble_game::identity::TileId;
    use scrabble_game::protocol::{BoardTile, PublicPlayer, PublicTurn, RackTile, ScoreChange};
    let mut snapshot = read(&started([7; 32], 100), 1);
    let letter = '\u{10400}'; // A four-byte uppercase alphabetic scalar, two UTF-16 code units.
    snapshot.public.board = (0..225)
        .map(|index| BoardTile {
            coordinate: Coordinate::new(index / 15, index % 15).unwrap(),
            letter,
            is_blank: true,
        })
        .collect();
    snapshot.public.players = (0..4)
        .map(|index| PublicPlayer {
            id: PlayerId::new(u64::MAX - index),
            display_name: letter.to_string().repeat(64),
            score: i32::MIN,
            rack_count: 15,
            connected: Some(false),
        })
        .collect();
    snapshot.public.history = (0..MAX_HISTORY_TURNS)
        .map(|index| PublicTurn {
            turn: TurnId::new(u64::MAX - index as u64),
            player_id: PlayerId::new(u64::MAX),
            action: TurnAction::Commit {
                words: vec![letter.to_string().repeat(15); 16],
                move_score: u32::MAX,
                blank_count: 15,
            },
            scores: snapshot
                .public
                .players
                .iter()
                .map(|player| ScoreChange {
                    player_id: player.id,
                    delta: -4294967295,
                    score: i32::MIN,
                })
                .collect(),
        })
        .collect();
    snapshot.own_rack.tiles = (0..15)
        .map(|_| RackTile {
            id: TileId::new(u64::MAX),
            face: TileFace::Letter { letter },
        })
        .collect();
    let configuration = &mut snapshot.public.configuration;
    configuration.identity.name = "n".repeat(64);
    configuration.identity.revision = "v".repeat(192);
    configuration.dictionary = configuration.identity.clone();
    configuration.tiles = (0x10400..=0x2ffff)
        .filter_map(char::from_u32)
        .filter(|letter| letter.is_alphabetic())
        .take(64)
        .map(|letter| scrabble_game::ruleset::TileDefinition {
            face: TileFace::Letter { letter },
            count: 3,
            value: u16::MAX,
        })
        .collect();
    configuration.tiles[0].count = 11;
    configuration.premiums = vec![scrabble_game::ruleset::Premium::TripleLetter; 225];
    configuration.rack_size = 15;
    configuration.validate().unwrap();
    snapshot.public.ruleset = configuration.identity.clone();
    snapshot.public.dictionary = configuration.dictionary.clone();
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    assert!(
        bytes.len() <= game_server::protocol::MAX_SNAPSHOT_PAYLOAD_BYTES,
        "{}-byte public payload exceeds native limit",
        bytes.len()
    );
    let frame = game_server::protocol::SnapshotFrame {
        tick: 0,
        state_hash: game_server::protocol::snapshot_hash(0, &bytes),
        payload: bytes,
    };
    assert!(game_server::protocol::encode_snapshot(&frame).is_ok());
}

#[test]
fn offline_report_verifies_persisted_native_evidence_and_excludes_private_fields() {
    use scrabble_server::{
        config::ServerConfig,
        factory::{MatchFactory, game_id},
        replay::inspect_replay,
    };
    use std::collections::BTreeMap;
    let directory = tempfile::tempdir().unwrap();
    let config = ServerConfig::from_values(&BTreeMap::from([
        ("SCRABBLE_MATCH_IDS".into(), "alpha".into()),
        ("SCRABBLE_TEST_SEED".into(), "07".repeat(32)),
        (
            "SCRABBLE_SEED_FILE".into(),
            directory.path().join("seed").to_str().unwrap().into(),
        ),
        (
            "SCRABBLE_RECOVERY_DIR".into(),
            directory.path().join("recovery").to_str().unwrap().into(),
        ),
    ]))
    .unwrap();
    let factory = MatchFactory::new(&config).unwrap();
    let id = game_server::MatchId::new("alpha").unwrap();
    let mut runtime = MatchRuntime::new_with_replay_capture(factory.create(&id).unwrap(), 100);
    runtime.admit(ReconnectToken([0xab; 16])).unwrap();
    runtime.admit(ReconnectToken([0xcd; 16])).unwrap();
    let command = CommandEnvelope {
        version: ProtocolVersion,
        game_id: game_id(&id),
        player_id: PlayerId::new(1),
        sequence: 1,
        expected_turn: TurnId::new(0),
        command: Command::Start {},
    };
    runtime
        .submit_command(1, 1, 1, &serde_json::to_vec(&command).unwrap())
        .unwrap();
    for turn in 0..6 {
        let actor = 1 + turn % 2;
        let sequence = if actor == 1 {
            2 + turn / 2
        } else {
            1 + turn / 2
        };
        let pass = CommandEnvelope {
            version: ProtocolVersion,
            game_id: game_id(&id),
            player_id: PlayerId::new(u64::from(actor)),
            sequence,
            expected_turn: TurnId::new(u64::from(turn)),
            command: Command::Pass {},
        };
        runtime
            .submit_command(actor, 1, sequence, &serde_json::to_vec(&pass).unwrap())
            .unwrap();
    }
    runtime.freeze_for_recovery();
    let canonical = runtime.snapshot().unwrap();
    let path = directory.path().join("image");
    runtime
        .recovery_image()
        .unwrap()
        .write_atomic(&path)
        .unwrap();
    let image = game_server::RecoveryImage::read_file(&path).unwrap();
    let report = inspect_replay(&factory, &id, &image).unwrap();
    assert_eq!(
        report.canonical_hash,
        format!("{:016x}", canonical.state_hash)
    );
    assert_eq!(report.records_verified, image.replay.records().len());
    assert!(report.checkpoints_verified > 0);
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["game"]["history"].as_array().unwrap().len(), 6);
    assert_eq!(json["game"]["phase"]["kind"], "finished");
    assert_eq!(
        json["game"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["game_id", "history", "phase"]
    );
    let encoded = serde_json::to_string(&json).unwrap();
    for secret in [
        "rack",
        "bag",
        "reconnect",
        "seed",
        &"ab".repeat(16),
        &"cd".repeat(16),
    ] {
        assert!(!encoded.contains(secret));
    }
    let wrong = game_server::MatchId::new("beta").unwrap();
    assert!(inspect_replay(&factory, &wrong, &image).is_err());
}

#[test]
fn largest_supported_private_checkpoint_stays_inside_native_byte_limit() {
    use game_server::GameSimulation;
    use scrabble_game::{
        ruleset::{Premium, TileDefinition},
        state::GameState,
    };
    let mut rules = english_fixture();
    rules.identity.name = "n".repeat(64);
    rules.identity.revision = "v".repeat(192);
    rules.dictionary = rules.identity.clone();
    rules.tiles = (0x10400..=0x2ffff)
        .filter_map(char::from_u32)
        .filter(|letter| letter.is_alphabetic())
        .take(64)
        .map(|letter| TileDefinition {
            face: TileFace::Letter { letter },
            count: 3,
            value: u16::MAX,
        })
        .collect();
    rules.tiles[0].count = 11;
    rules.rack_size = 15;
    rules.premiums = vec![Premium::TripleLetter; 225];
    let mut state = GameState::new(GameId::new(u64::MAX), rules.clone(), [7; 32]).unwrap();
    for actor in 1..=4 {
        state
            .add_player(PlayerId::new(actor), "\u{10400}".repeat(32))
            .unwrap();
    }
    state.deal_initial_racks().unwrap();
    let placements = state
        .rack(PlayerId::new(1))
        .unwrap()
        .iter()
        .take(2)
        .enumerate()
        .map(|(index, tile)| Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
            blank_as: None,
        })
        .collect::<Vec<_>>();
    state.place_tiles(PlayerId::new(1), &placements).unwrap();
    let mut game: serde_json::Value =
        serde_json::from_slice(&state.canonical_bytes().unwrap()).unwrap();
    let committed = game["board"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| !entry.is_null())
        .unwrap()
        .clone();
    let mut board = vec![committed; 200];
    board.extend(vec![serde_json::Value::Null; 25]);
    game["board"] = serde_json::json!(board);
    game["bag"] = serde_json::json!([]);
    for player in game["players"].as_array_mut().unwrap() {
        player["id"] = serde_json::json!(u32::MAX.to_string());
        player["rack"] = serde_json::json!([]);
        player["score"] = serde_json::json!(i32::MIN);
    }
    let mut public = read(&started([7; 32], 100), 1).public;
    public.history = (0..MAX_HISTORY_TURNS)
        .map(|index| scrabble_game::protocol::PublicTurn {
            turn: TurnId::new(u64::MAX - index as u64),
            player_id: PlayerId::new(u64::MAX),
            action: TurnAction::Commit {
                words: vec!["\u{10400}".repeat(15); 16],
                move_score: u32::MAX,
                blank_count: 15,
            },
            scores: (0..4)
                .map(|index| scrabble_game::protocol::ScoreChange {
                    player_id: PlayerId::new(u64::MAX - index),
                    delta: -4294967295,
                    score: i32::MIN,
                })
                .collect(),
        })
        .collect();
    game["history"] = serde_json::to_value(&public.history).unwrap();
    game["revision"] = serde_json::json!(u64::MAX.to_string());
    game["phase"]["turn"] = serde_json::json!(u64::MAX.to_string());
    game["phase"]["active_player"] = serde_json::json!(u32::MAX.to_string());
    // Use the real adapter envelope. Synthetic full history/board sizes intentionally
    // overestimate simultaneous legal play; this checks serialization capacity, not recovery validity.
    let simulation = ScrabbleSimulation::new(GameId::new(u64::MAX), rules, [7; 32]).unwrap();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&simulation.snapshot().unwrap().payload).unwrap();
    envelope["game"] = game;
    envelope["admitted"] = serde_json::json!([u32::MAX - 3, u32::MAX - 2, u32::MAX - 1, u32::MAX]);
    envelope["last_admitted_player"] = serde_json::json!(u32::MAX);
    envelope["lifecycle"] = serde_json::json!({"created_at":u64::MAX,"finished_at_tick":u64::MAX});
    envelope["board"] = serde_json::json!({"owner":u32::MAX});
    let bytes = serde_json::to_vec(&envelope).unwrap();
    assert!(
        bytes.len() <= game_server::MAX_SNAPSHOT_PAYLOAD_BYTES,
        "{}-byte checkpoint exceeds native limit",
        bytes.len()
    );
    let frame = game_server::protocol::SnapshotFrame {
        tick: 0,
        state_hash: game_server::protocol::snapshot_hash(0, &bytes),
        payload: bytes,
    };
    assert!(game_server::protocol::encode_snapshot(&frame).is_ok());
}
