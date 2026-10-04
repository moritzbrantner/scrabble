use game_server::{MatchId, MatchRuntime, ReconnectToken, ReplayLog, verify_replay};
use scrabble_game::{
    identity::{Coordinate, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement, PlayerSnapshot},
    ruleset::{Ruleset, TileFace, german_fixture},
};
use scrabble_server::{
    config::ServerConfig,
    factory::{MatchFactory, game_id},
};
use std::{collections::BTreeMap, fs, path::Path};

fn configuration(directory: &Path) -> (BTreeMap<String, String>, ServerConfig) {
    fs::write(
        directory.join("rules.json"),
        serde_json::to_vec(&german_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.join("words.txt"),
        include_str!("../../scrabble-game/fixtures/de-words.txt"),
    )
    .unwrap();
    let values = BTreeMap::from([
        (
            "SCRABBLE_RULESET_FILE".into(),
            directory.join("rules.json").to_str().unwrap().into(),
        ),
        (
            "SCRABBLE_DICTIONARY_FILE".into(),
            directory.join("words.txt").to_str().unwrap().into(),
        ),
        (
            "SCRABBLE_DICTIONARY_NAME".into(),
            "scrabble-de-authored-fixture".into(),
        ),
        ("SCRABBLE_DICTIONARY_REVISION".into(), "1".into()),
        (
            "SCRABBLE_SEED_FILE".into(),
            directory.join("seed").to_str().unwrap().into(),
        ),
        (
            "SCRABBLE_RECOVERY_DIR".into(),
            directory.join("recovery").to_str().unwrap().into(),
        ),
        ("SCRABBLE_TEST_SEED".into(), "07".repeat(32)),
    ]);
    let config = ServerConfig::from_values(&values).unwrap();
    (values, config)
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
fn deal(
    factory: &MatchFactory,
    faces: &[TileFace],
) -> (
    MatchId,
    MatchRuntime<scrabble_server::simulation::ScrabbleSimulation>,
    PlayerSnapshot,
) {
    (0..10_000)
        .find_map(|index| {
            let id = MatchId::new(format!("variant-{index}")).unwrap();
            let mut runtime =
                MatchRuntime::new_with_replay_capture(factory.create(&id).unwrap(), 100);
            runtime.admit(ReconnectToken([1; 16])).unwrap();
            runtime.admit(ReconnectToken([2; 16])).unwrap();
            runtime
                .submit_command(1, 1, 1, &payload(&id, 1, Command::Start {}))
                .unwrap();
            let snapshot: PlayerSnapshot =
                serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
            faces
                .iter()
                .all(|face| {
                    snapshot
                        .own_rack
                        .tiles
                        .iter()
                        .any(|tile| tile.face == *face)
                })
                .then_some((id, runtime, snapshot))
        })
        .expect("bounded native search finds the authored deal")
}
#[test]
fn unicode_ruleset_scores_blanks_and_replays_only_under_the_original_immutable_content() {
    let directory = tempfile::tempdir().unwrap();
    let (_, config) = configuration(directory.path());
    let factory = MatchFactory::new(&config).unwrap();
    let (id, mut runtime, snapshot) = deal(
        &factory,
        &[TileFace::Blank {}, TileFace::Letter { letter: 'L' }],
    );
    let rules = &snapshot.public.configuration;
    assert_eq!(rules.board_size, 7);
    assert_eq!(snapshot.own_rack.tiles.len(), 3);
    assert_eq!(rules.identity, snapshot.public.ruleset);
    assert_eq!(rules.dictionary, snapshot.public.dictionary);
    assert!(rules.identity.revision.starts_with("1+rules-sha256:"));
    let blank = snapshot
        .own_rack
        .tiles
        .iter()
        .find(|tile| tile.face == TileFace::Blank {})
        .unwrap();
    let letter = snapshot
        .own_rack
        .tiles
        .iter()
        .find(|tile| tile.face == TileFace::Letter { letter: 'L' })
        .unwrap();
    let placements = vec![
        Placement {
            tile_id: blank.id,
            coordinate: Coordinate::new(3, 3).unwrap(),
            blank_as: Some('Ö'),
        },
        Placement {
            tile_id: letter.id,
            coordinate: Coordinate::new(3, 4).unwrap(),
            blank_as: None,
        },
    ];
    let before = runtime.snapshot().unwrap();
    let before_log = runtime.replay_log().unwrap().encode().unwrap();
    let mut invalid = placements.clone();
    invalid[0].blank_as = Some('X');
    assert!(
        runtime
            .submit_command(
                1,
                1,
                2,
                &payload(
                    &id,
                    2,
                    Command::Commit {
                        placements: invalid
                    }
                )
            )
            .is_err()
    );
    assert_eq!(runtime.snapshot().unwrap(), before);
    assert_eq!(runtime.replay_log().unwrap().encode().unwrap(), before_log);
    runtime
        .submit_command(1, 1, 2, &payload(&id, 2, Command::Commit { placements }))
        .unwrap();
    let after: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    assert_eq!(after.public.players[0].score, 8);
    assert_eq!(
        after
            .public
            .board
            .iter()
            .map(|tile| tile.letter)
            .collect::<String>(),
        "ÖL"
    );
    assert!(after.public.board[0].is_blank);
    assert_eq!(after.own_rack.tiles.len(), 3);
    assert_eq!(after.public.configuration, *rules);
    assert!(
        matches!(after.public.phase,Phase::Playing {active_player,..} if active_player==PlayerId::new(2))
    );
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
    let original: Ruleset =
        serde_json::from_slice(&fs::read(directory.path().join("rules.json")).unwrap()).unwrap();
    // Formatting alone does not change the canonical data fingerprint.
    fs::write(
        directory.path().join("rules.json"),
        serde_json::to_string_pretty(&original).unwrap(),
    )
    .unwrap();
    let equivalent = MatchFactory::new(&config).unwrap();
    assert_eq!(
        MatchRuntime::restore_from_recovery(equivalent.create(&id).unwrap(), recovery.clone())
            .unwrap()
            .snapshot()
            .unwrap(),
        expected
    );
    let mutations: [fn(&mut Ruleset); 6] = [
        |rules| rules.tiles[0].value += 1,
        |rules| rules.tiles[0].count += 1,
        |rules| rules.rack_size += 1,
        |rules| rules.bingo_bonus += 1,
        |rules| rules.premiums[0] = scrabble_game::ruleset::Premium::DoubleLetter,
        |rules| rules.scoreless_turn_limit += 1,
    ];
    for mutate in mutations {
        let mut changed = original.clone();
        mutate(&mut changed);
        fs::write(
            directory.path().join("rules.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        let changed_factory = MatchFactory::new(&config).unwrap();
        assert!(verify_replay(changed_factory.create(&id).unwrap(), &log).is_err());
        assert!(
            MatchRuntime::restore_from_recovery(
                changed_factory.create(&id).unwrap(),
                recovery.clone()
            )
            .is_err()
        );
    }
    // The factory retains its immutable validated profile after the deployment file changes.
    assert_eq!(
        verify_replay(factory.create(&id).unwrap(), &log)
            .unwrap()
            .final_snapshot,
        expected
    );
}
#[test]
fn configured_rack_size_and_bingo_bonus_drive_the_same_native_commit_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let (_, config) = configuration(directory.path());
    let factory = MatchFactory::new(&config).unwrap();
    let (id, mut runtime, snapshot) = deal(
        &factory,
        &['B', 'Ä', 'R'].map(|letter| TileFace::Letter { letter }),
    );
    let placements = ['B', 'Ä', 'R']
        .into_iter()
        .enumerate()
        .map(|(index, letter)| Placement {
            tile_id: snapshot
                .own_rack
                .tiles
                .iter()
                .find(|tile| tile.face == TileFace::Letter { letter })
                .unwrap()
                .id,
            coordinate: Coordinate::new(3, 3 + index as u8).unwrap(),
            blank_as: None,
        })
        .collect();
    runtime
        .submit_command(1, 1, 2, &payload(&id, 2, Command::Commit { placements }))
        .unwrap();
    let snapshot: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    assert_eq!(snapshot.public.players[0].score, 52); // (B3 + Ä6×2 + R1)×2 + configured bonus20.
    assert_eq!(
        snapshot.public.history[0].action,
        scrabble_game::protocol::TurnAction::Commit {
            words: vec!["BÄR".into()],
            move_score: 52,
            blank_count: 0
        }
    );
    assert_eq!(snapshot.own_rack.tiles.len(), 3);
}
#[test]
fn invalid_profiles_fail_before_a_match_or_private_seed_can_be_initialized() {
    let directory = tempfile::tempdir().unwrap();
    let (values, config) = configuration(directory.path());
    let mut missing_dictionary = values.clone();
    missing_dictionary.remove("SCRABBLE_DICTIONARY_FILE");
    assert!(ServerConfig::from_values(&missing_dictionary).is_err());
    let original = serde_json::to_value(german_fixture()).unwrap();
    let mut cases = Vec::new();
    for (key, value) in [
        ("rack_size", serde_json::json!(0)),
        ("board_size", serde_json::json!(6)),
        ("premiums", serde_json::json!([])),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        cases.push(invalid);
    }
    let mut missing = original.clone();
    missing.as_object_mut().unwrap().remove("bingo_bonus");
    cases.push(missing);
    let mut duplicate = original.clone();
    let face = duplicate["tiles"][0].clone();
    duplicate["tiles"].as_array_mut().unwrap().push(face);
    cases.push(duplicate);
    let mut blank = original.clone();
    blank["tiles"].as_array_mut().unwrap().last_mut().unwrap()["value"] = serde_json::json!(1);
    cases.push(blank);
    let mut oversized = original.clone();
    oversized["tiles"][0]["count"] = serde_json::json!(201);
    cases.push(oversized);
    let mut dictionary = original.clone();
    dictionary["dictionary"]["name"] = serde_json::json!("wrong-list");
    cases.push(dictionary);
    for invalid in cases {
        fs::write(
            directory.path().join("rules.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert!(MatchFactory::new(&config).is_err());
        assert!(!config.seed_file.exists());
    }
    fs::write(
        directory.path().join("rules.json"),
        vec![b' '; 128 * 1024 + 1],
    )
    .unwrap();
    assert!(MatchFactory::new(&config).is_err());
    assert!(!config.seed_file.exists());
    fs::write(
        directory.path().join("rules.json"),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    fs::write(directory.path().join("words.txt"), "AT\n").unwrap(); // T is outside this configured alphabet.
    assert!(MatchFactory::new(&config).is_err());
    assert!(!config.seed_file.exists());
}
