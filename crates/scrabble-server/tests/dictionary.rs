use game_server::{
    GameSimulation, MatchId, MatchRuntime, ReconnectToken, ReplayLog, verify_replay,
};
use scrabble_game::{
    identity::{Coordinate, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Placement, PlayerSnapshot},
    ruleset::TileFace,
};
use scrabble_server::{
    config::ServerConfig,
    factory::{MatchFactory, game_id},
};
use std::{collections::BTreeMap, fs};

fn values(directory: &std::path::Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "SCRABBLE_SEED_FILE".into(),
            directory.join("seed").to_str().unwrap().into(),
        ),
        (
            "SCRABBLE_RECOVERY_DIR".into(),
            directory.join("recovery").to_str().unwrap().into(),
        ),
        (
            "SCRABBLE_DICTIONARY_FILE".into(),
            directory.join("words.txt").to_str().unwrap().into(),
        ),
        ("SCRABBLE_DICTIONARY_NAME".into(), "owned-test-list".into()),
        ("SCRABBLE_DICTIONARY_REVISION".into(), "2026.1".into()),
        ("SCRABBLE_TEST_SEED".into(), "07".repeat(32)),
    ])
}
fn payload(id: &MatchId, sequence: u32, command: Command) -> Vec<u8> {
    serde_json::to_vec(&CommandEnvelope {
        version: ProtocolVersion,
        game_id: game_id(id),
        player_id: PlayerId::new(1),
        sequence,
        expected_turn: TurnId::new(0),
        command,
    })
    .unwrap()
}

#[test]
fn deployment_word_list_is_immutable_content_identified_and_used_during_replay_and_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let values = values(directory.path());
    fs::write(directory.path().join("words.txt"), "TA\nA\n").unwrap();
    let config = ServerConfig::from_values(&values).unwrap();
    let factory = MatchFactory::new(&config).unwrap();
    let (id, mut runtime, placements) = (0..1000)
        .find_map(|index| {
            let id = MatchId::new(format!("table-{index}")).unwrap();
            let mut runtime =
                MatchRuntime::new_with_replay_capture(factory.create(&id).unwrap(), 100);
            runtime.admit(ReconnectToken([1; 16])).unwrap();
            runtime.admit(ReconnectToken([2; 16])).unwrap();
            runtime
                .submit_command(1, 1, 1, &payload(&id, 1, Command::Start {}))
                .unwrap();
            let snapshot: PlayerSnapshot =
                serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
            let placements: Option<Vec<_>> = ['T', 'A']
                .into_iter()
                .enumerate()
                .map(|(index, letter)| {
                    snapshot
                        .own_rack
                        .tiles
                        .iter()
                        .find(|tile| tile.face == (TileFace::Letter { letter }))
                        .map(|tile| Placement {
                            tile_id: tile.id,
                            coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
                            blank_as: None,
                        })
                })
                .collect();
            placements.map(|placements| (id, runtime, placements))
        })
        .expect("deterministic TA deal");
    let initial = factory.create(&id).unwrap().snapshot().unwrap();
    runtime
        .submit_command(1, 1, 2, &payload(&id, 2, Command::Commit { placements }))
        .unwrap();
    let own: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    assert_eq!(
        own.public
            .board
            .iter()
            .map(|tile| tile.letter)
            .collect::<String>(),
        "TA"
    );
    assert_eq!(own.public.players[0].score, 4);
    assert_eq!(own.public.dictionary.name, "owned-test-list");
    assert!(own.public.dictionary.revision.starts_with("2026.1+sha256:"));
    assert!(
        own.public
            .ruleset
            .revision
            .starts_with("1+dictionary-sha256:")
    );
    let scoped: serde_json::Value =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    assert!(scoped.get("words").is_none());
    assert!(scoped["public"]["dictionary"].get("words").is_none());
    runtime.advance_tick().unwrap();
    let expected = runtime.snapshot().unwrap();
    let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(factory.create(&id).unwrap(), &log)
            .unwrap()
            .final_snapshot,
        expected
    );
    runtime.freeze_for_recovery();
    let recovery = runtime.recovery_image().unwrap();
    fs::write(directory.path().join("words.txt"), "A\r\nTA\r\nTA\r\n").unwrap();
    let equivalent = MatchFactory::new(&config).unwrap();
    assert_eq!(equivalent.create(&id).unwrap().snapshot().unwrap(), initial);
    let restored =
        MatchRuntime::restore_from_recovery(equivalent.create(&id).unwrap(), recovery.clone())
            .unwrap();
    assert_eq!(restored.snapshot().unwrap(), expected);
    fs::write(directory.path().join("words.txt"), "A\n").unwrap();
    let changed = MatchFactory::new(&config).unwrap();
    assert_ne!(changed.create(&id).unwrap().snapshot().unwrap(), initial);
    assert!(MatchRuntime::restore_from_recovery(changed.create(&id).unwrap(), recovery).is_err());
    // Existing factories retain the already-validated list despite file changes.
    assert_eq!(factory.create(&id).unwrap().snapshot().unwrap(), initial);
    assert_eq!(
        verify_replay(factory.create(&id).unwrap(), &log)
            .unwrap()
            .final_snapshot,
        expected
    );
}

#[test]
fn dictionary_configuration_and_content_fail_closed_before_private_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let original = values(directory.path());
    for missing in [
        "SCRABBLE_DICTIONARY_FILE",
        "SCRABBLE_DICTIONARY_NAME",
        "SCRABBLE_DICTIONARY_REVISION",
    ] {
        let mut values = original.clone();
        values.remove(missing);
        assert!(ServerConfig::from_values(&values).is_err());
    }
    for (key, value) in [
        ("SCRABBLE_DICTIONARY_NAME", ""),
        ("SCRABBLE_DICTIONARY_REVISION", "bad\nrevision"),
        ("SCRABBLE_DICTIONARY_FILE", ""),
    ] {
        let mut values = original.clone();
        values.insert(key.into(), value.into());
        assert!(ServerConfig::from_values(&values).is_err());
    }
    let config = ServerConfig::from_values(&original).unwrap();
    assert!(MatchFactory::new(&config).is_err());
    for content in [b"".as_slice(), b"at\n", b"AT1\n", &[0xff, 0xfe]] {
        fs::write(directory.path().join("words.txt"), content).unwrap();
        assert!(MatchFactory::new(&config).is_err());
        assert!(!config.seed_file.exists());
    }
    fs::write(directory.path().join("words.txt"), "AT\n").unwrap();
    let before = MatchFactory::new(&config)
        .unwrap()
        .create(&MatchId::new("table-1").unwrap())
        .unwrap()
        .snapshot()
        .unwrap();
    for (key, value) in [
        ("SCRABBLE_DICTIONARY_NAME", "another-list"),
        ("SCRABBLE_DICTIONARY_REVISION", "2026.2"),
    ] {
        let mut values = original.clone();
        values.insert(key.into(), value.into());
        let config = ServerConfig::from_values(&values).unwrap();
        let after = MatchFactory::new(&config)
            .unwrap()
            .create(&MatchId::new("table-1").unwrap())
            .unwrap()
            .snapshot()
            .unwrap();
        assert_ne!(before, after);
    }
}
