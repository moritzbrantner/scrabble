use game_server::{CommandOutcome, MatchId, MatchRuntime, ReconnectToken, verify_replay};
use scrabble_game::{
    dictionary::DictionaryError,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TileId, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement, PlayerSnapshot},
};
use scrabble_server::{
    config::ServerConfig,
    factory::{MatchFactory, game_id},
    simulation::ScrabbleSimulation,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn envelope(game: GameId, player: u64, sequence: u32, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: game,
        player_id: PlayerId::new(player),
        sequence,
        expected_turn: TurnId::new(0),
        command,
    }
}
fn projected(runtime: &MatchRuntime<ScrabbleSimulation>, player: u32) -> PlayerSnapshot {
    serde_json::from_slice(&runtime.snapshot_for(player).unwrap().payload).unwrap()
}
fn setup() -> (
    tempfile::TempDir,
    MatchFactory,
    MatchId,
    MatchRuntime<ScrabbleSimulation>,
) {
    let directory = tempfile::tempdir().unwrap();
    let config = ServerConfig::from_values(&BTreeMap::from([
        ("SCRABBLE_MATCH_IDS".into(), String::new()),
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
    let request_id = "ab".repeat(16);
    let id = factory.board_creation_id(&request_id, 1000).unwrap();
    let game = game_id(&id);
    let mut runtime = MatchRuntime::new_with_replay_capture(factory.create(&id).unwrap(), 100);
    for actor in 1..=3 {
        runtime.admit(ReconnectToken([actor; 16])).unwrap();
    }
    for (player, sequence, command) in [
        (1, 1, Command::ClaimBoard { request_id }),
        (
            2,
            1,
            Command::SetName {
                display_name: "Ada".into(),
            },
        ),
        (
            3,
            1,
            Command::SetName {
                display_name: "Lin".into(),
            },
        ),
        (1, 2, Command::Start {}),
    ] {
        runtime
            .submit_command(
                player,
                1,
                sequence,
                &serde_json::to_vec(&envelope(game, u64::from(player), sequence, command)).unwrap(),
            )
            .unwrap();
    }
    (directory, factory, id, runtime)
}

#[test]
fn forged_and_malformed_messages_preserve_canonical_state_scoped_views_and_replay() {
    let (_directory, factory, id, mut runtime) = setup();
    let game = game_id(&id);
    let views: Vec<_> = (1..=3)
        .map(|player| runtime.snapshot_for(player).unwrap())
        .collect();
    let canonical = runtime.snapshot().unwrap();
    let records = runtime.replay_log().unwrap().records().len();
    let other_tile = projected(&runtime, 3).own_rack.tiles[0].id;
    let value: Value = serde_json::from_slice(&canonical.payload).unwrap();
    let bag_tile =
        TileId::try_from(value["game"]["bag"][0]["id"].as_str().unwrap().to_owned()).unwrap();
    let mut attacks = Vec::new();
    for tile_id in [other_tile, bag_tile, TileId::new(u64::MAX)] {
        let placement = Placement {
            tile_id,
            coordinate: Coordinate::new(7, 7).unwrap(),
            blank_as: None,
        };
        for command in [
            Command::Preview {
                placements: vec![placement.clone()],
            },
            Command::Commit {
                placements: vec![placement.clone()],
            },
            Command::Exchange {
                tile_ids: vec![tile_id],
            },
        ] {
            attacks.push((
                2,
                2,
                serde_json::to_vec(&envelope(game, 2, 2, command)).unwrap(),
            ));
        }
    }
    let pass = envelope(game, 2, 2, Command::Pass {});
    let mut wrong_identity = pass.clone();
    wrong_identity.player_id = PlayerId::new(3);
    let mut wrong_game = pass.clone();
    wrong_game.game_id = GameId::new(0);
    let mut wrong_turn = pass.clone();
    wrong_turn.expected_turn = TurnId::new(99);
    let mut wrong_sequence = pass.clone();
    wrong_sequence.sequence = 3;
    for command in [wrong_identity, wrong_game, wrong_turn, wrong_sequence] {
        attacks.push((2, 2, serde_json::to_vec(&command).unwrap()));
    }
    attacks.push((
        1,
        3,
        serde_json::to_vec(&envelope(game, 2, 3, Command::Pass {})).unwrap(),
    ));
    attacks.push((
        1,
        3,
        serde_json::to_vec(&envelope(game, 1, 3, Command::Pass {})).unwrap(),
    ));
    attacks.push((
        3,
        2,
        serde_json::to_vec(&envelope(game, 3, 2, Command::Pass {})).unwrap(),
    ));
    for row in [
        json!(-1),
        json!(15),
        json!(255),
        json!(u64::MAX),
        json!("private-marker"),
    ] {
        let mut command = serde_json::to_value(&pass).unwrap();
        command["command"] = json!({"kind":"preview","placements":[{
            "tile_id":"1","coordinate":{"row":row,"column":7},"blank_as":null
        }]});
        attacks.push((2, 2, serde_json::to_vec(&command).unwrap()));
    }
    for command in [
        json!({"kind":"get_rack","player_id":"3"}),
        json!({"kind":"snapshot","scope":"canonical"}),
        json!({"kind":"pass","score":999}),
    ] {
        let mut value = serde_json::to_value(&pass).unwrap();
        value["command"] = command;
        attacks.push((2, 2, serde_json::to_vec(&value).unwrap()));
    }
    attacks.extend([
        (2, 2, vec![b' '; game_server::MAX_COMMAND_PAYLOAD_BYTES + 1]),
        (2, 2, vec![0xff]),
        (2, 2, b"{\"version\":1,".to_vec()),
    ]);
    for (actor, sequence, payload) in attacks {
        assert!(
            runtime
                .submit_command(actor, 1, sequence, &payload)
                .is_err()
        );
        assert_eq!(runtime.snapshot().unwrap(), canonical);
        assert_eq!(runtime.replay_log().unwrap().records().len(), records);
        for player in 1..=3 {
            assert_eq!(
                runtime.snapshot_for(player).unwrap(),
                views[(player - 1) as usize]
            );
        }
    }
    // Rejection consumed neither native nor domain sequence. A legal action still completes.
    runtime
        .submit_command(2, 1, 2, &serde_json::to_vec(&pass).unwrap())
        .unwrap();
    assert_eq!(
        projected(&runtime, 2).public.phase,
        Phase::Playing {
            active_player: PlayerId::new(3),
            turn: TurnId::new(1)
        }
    );
    assert_eq!(
        verify_replay(factory.create(&id).unwrap(), runtime.replay_log().unwrap())
            .unwrap()
            .final_snapshot,
        runtime.snapshot().unwrap()
    );
}

#[test]
fn board_and_phone_projections_have_exact_public_boundaries_and_rotated_epochs_stay_fenced() {
    let (_directory, _factory, id, mut runtime) = setup();
    let board = projected(&runtime, 1);
    let first = projected(&runtime, 2);
    let second = projected(&runtime, 3);
    assert!(board.own_rack.tiles.is_empty());
    assert_eq!(board.public, first.public);
    assert_eq!(board.public, second.public);
    let first_ids: BTreeSet<_> = first.own_rack.tiles.iter().map(|tile| tile.id).collect();
    assert!(
        second
            .own_rack
            .tiles
            .iter()
            .all(|tile| !first_ids.contains(&tile.id))
    );
    let canonical: Value = serde_json::from_slice(&runtime.snapshot().unwrap().payload).unwrap();
    for (actor, snapshot) in [(1, board), (2, first), (3, second)] {
        if actor != 1 {
            let actor_id = actor.to_string();
            let owner = canonical["game"]["players"]
                .as_array()
                .unwrap()
                .iter()
                .find(|player| player["id"].as_str() == Some(actor_id.as_str()))
                .unwrap();
            assert_eq!(
                owner["rack"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|tile| &tile["id"])
                    .collect::<Vec<_>>(),
                snapshot
                    .own_rack
                    .tiles
                    .iter()
                    .map(|tile| serde_json::to_value(tile.id).unwrap())
                    .collect::<Vec<_>>()
                    .iter()
                    .collect::<Vec<_>>()
            );
        }
        let value = serde_json::to_value(snapshot).unwrap();
        assert_eq!(
            value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["own_rack", "public"]
        );
        assert_eq!(value["own_rack"]["player_id"], actor.to_string());
        let public = value["public"].as_object().unwrap();
        assert_eq!(
            public.keys().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "board",
                "configuration",
                "dictionary",
                "game_id",
                "history",
                "host",
                "phase",
                "players",
                "preview",
                "remaining_tiles",
                "revision",
                "ruleset",
                "version"
            ]
        );
        for player in public["players"].as_array().unwrap() {
            assert_eq!(
                player
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                vec!["connected", "display_name", "id", "rack_count", "score"]
            );
        }
    }
    assert!(runtime.snapshot_for(99).is_err());
    assert!(runtime.disconnect(2, 1));
    let fresh = runtime
        .reconnect(ReconnectToken([2; 16]), ReconnectToken([4; 16]))
        .unwrap();
    assert_eq!(fresh.player_id, 2);
    assert_eq!(fresh.connection_epoch, 2);
    let canonical = runtime.snapshot().unwrap();
    let records = runtime.replay_log().unwrap().records().len();
    let pass = serde_json::to_vec(&envelope(game_id(&id), 2, 2, Command::Pass {})).unwrap();
    assert!(runtime.submit_command(2, 1, 2, &pass).is_err());
    assert!(!runtime.disconnect(2, 1));
    assert!(
        runtime
            .reconnect(ReconnectToken([2; 16]), ReconnectToken([5; 16]))
            .is_err()
    );
    assert_eq!(
        runtime.submit_command(2, 2, 1, &pass).unwrap(),
        CommandOutcome::IgnoredStale
    );
    assert_eq!(runtime.snapshot().unwrap(), canonical);
    assert_eq!(runtime.replay_log().unwrap().records().len(), records);
    runtime.submit_command(2, 2, 2, &pass).unwrap();
}

#[test]
fn private_domain_diagnostics_are_redacted_while_explicit_encoding_retains_authority() {
    let (_directory, _factory, id, runtime) = setup();
    let snapshot = projected(&runtime, 2);
    let claim = envelope(
        game_id(&id),
        1,
        1,
        Command::ClaimBoard {
            request_id: "ab".repeat(16),
        },
    );
    assert!(!format!("{claim:?}").contains(&"ab".repeat(16)));
    assert_eq!(
        CommandEnvelope::decode(&serde_json::to_vec(&claim).unwrap()).unwrap(),
        claim
    );
    assert_eq!(
        format!("{:?}", snapshot.own_rack.tiles[0]),
        "RackTile { .. }"
    );
    let diagnostic = format!("{:?}", snapshot.own_rack);
    assert!(!diagnostic.contains("Letter"));
    assert!(!diagnostic.contains("TileId"));
    let private_error = DictionaryError::InvalidWords {
        words: vec!["PRIVATEWORD".into()],
    };
    assert!(!format!("{private_error:?} {private_error}").contains("PRIVATEWORD"));
    assert_eq!(
        serde_json::to_value(&private_error).unwrap()["words"],
        json!(["PRIVATEWORD"])
    );
    for invalid in [
        json!({"kind":"pass","private-marker":true}),
        json!({"kind":"private-marker"}),
    ] {
        let mut value = serde_json::to_value(&claim).unwrap();
        value["command"] = invalid;
        let error = CommandEnvelope::decode(&serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert_eq!(error.to_string(), "invalid command encoding");
        assert!(!format!("{error:?}").contains("private-marker"));
    }
}
