use scrabble_game::identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TileId, TurnId};
use scrabble_game::protocol::{
    Command, CommandEnvelope, MAX_COMMAND_BYTES, Phase, PublicPlayer, PublicSnapshot,
};
use scrabble_game::ruleset::{Premium, TileFace, english_fixture};
use serde_json::json;

fn pass() -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(u64::MAX),
        player_id: PlayerId::new(1),
        sequence: 12,
        expected_turn: TurnId::new(3),
        command: Command::Pass {},
    }
}

#[test]
fn v1_command_roundtrips_without_identifier_rounding() {
    let encoded = serde_json::to_vec(&pass()).unwrap();
    assert_eq!(CommandEnvelope::decode(&encoded).unwrap(), pass());
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(value["game_id"], "18446744073709551615");
    assert_eq!(value["version"], 1);
    assert_eq!(value["command"], json!({"kind":"pass"}));
}

#[test]
fn every_command_has_an_unambiguous_v1_encoding() {
    for command in [
        Command::Start {},
        Command::Pass {},
        Command::Preview { placements: vec![] },
        Command::Commit { placements: vec![] },
        Command::Exchange {
            tile_ids: vec![TileId::new(19)],
        },
    ] {
        let envelope = CommandEnvelope { command, ..pass() };
        assert_eq!(
            CommandEnvelope::decode(&serde_json::to_vec(&envelope).unwrap()).unwrap(),
            envelope
        );
    }
}

#[test]
fn unknown_version_fields_and_noncanonical_ids_fail_closed() {
    let original = serde_json::to_value(pass()).unwrap();
    for (key, invalid) in [
        ("version", json!(2)),
        ("game_id", json!(9_007_199_254_740_993_u64)),
        ("game_id", json!("01")),
        ("player_id", json!("-1")),
        ("expected_turn", json!("18446744073709551616")),
        ("sequence", json!(u64::MAX)),
        ("unknown", json!(true)),
    ] {
        let mut value = original.clone();
        value[key] = invalid;
        assert!(
            CommandEnvelope::decode(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{key}"
        );
    }
    let mut value = original;
    value["command"] = json!({"kind":"pass","tile_ids":["1"]});
    assert!(CommandEnvelope::decode(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn invalid_coordinates_and_bounded_commands_are_rejected_before_gameplay() {
    assert!(Coordinate::new(15, 0).is_err());
    assert!(Coordinate::new(0, 15).is_err());
    assert_eq!(Coordinate::new(14, 14).unwrap().column(), 14);
    let mut value = serde_json::to_value(pass()).unwrap();
    value["command"] = json!({"kind":"commit","placements":[{"tile_id":"1","coordinate":{"row":255,"column":0},"blank_as":null}]});
    assert!(CommandEnvelope::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    let large = CommandEnvelope {
        command: Command::Exchange {
            tile_ids: vec![TileId::new(1); 16],
        },
        ..pass()
    };
    assert!(CommandEnvelope::decode(&serde_json::to_vec(&large).unwrap()).is_err());
    assert!(CommandEnvelope::decode(&vec![b' '; MAX_COMMAND_BYTES + 1]).is_err());
}

#[test]
fn public_snapshot_has_no_private_fields_and_rejects_injected_racks() {
    let rules = english_fixture();
    let snapshot = PublicSnapshot {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        revision: TurnId::new(0),
        ruleset: rules.identity,
        dictionary: rules.dictionary,
        phase: Phase::Lobby {},
        board: vec![],
        players: vec![PublicPlayer {
            id: PlayerId::new(1),
            display_name: "Ada".into(),
            score: 0,
            rack_count: 0,
        }],
        remaining_tiles: 100,
        preview: None,
    };
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    assert!(encoded.get("racks").is_none());
    assert!(encoded.get("bag").is_none());
    assert!(encoded["players"][0].get("rack").is_none());
    assert_eq!(
        serde_json::from_value::<PublicSnapshot>(encoded.clone()).unwrap(),
        snapshot
    );
    encoded["players"][0]["rack"] = json!(["Q"]);
    assert!(serde_json::from_value::<PublicSnapshot>(encoded).is_err());
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    encoded["bag"] = json!(["Q"]);
    assert!(serde_json::from_value::<PublicSnapshot>(encoded).is_err());
}

#[test]
fn fixture_is_explicit_valid_and_offline() {
    let rules = english_fixture();
    assert!(rules.validate().is_ok());
    assert_eq!(
        rules
            .tiles
            .iter()
            .map(|tile| u32::from(tile.count))
            .sum::<u32>(),
        100
    );
    assert_eq!(rules.identity.name, "en-classic-fixture");
    assert_eq!(rules.dictionary.name, "scrabble-authored-fixture");
    assert_eq!(rules.premiums[7 * 15 + 7], Premium::DoubleWord);
    assert_eq!(
        rules
            .premiums
            .iter()
            .filter(|&&premium| premium == Premium::TripleWord)
            .count(),
        8
    );
    assert_eq!(
        rules
            .premiums
            .iter()
            .filter(|&&premium| premium == Premium::DoubleWord)
            .count(),
        17
    );
    assert_eq!(
        rules
            .premiums
            .iter()
            .filter(|&&premium| premium == Premium::TripleLetter)
            .count(),
        12
    );
    assert_eq!(
        rules
            .premiums
            .iter()
            .filter(|&&premium| premium == Premium::DoubleLetter)
            .count(),
        24
    );
    let words: Vec<_> = include_str!("../fixtures/words.txt").lines().collect();
    assert!(words.contains(&"CAT"));
    assert!(words.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn invalid_rulesets_do_not_reach_match_start() {
    let base = english_fixture();
    let mut invalid = base.clone();
    invalid.dictionary.revision.clear();
    assert!(invalid.validate().is_err());
    invalid = base.clone();
    invalid.premiums.pop();
    assert!(invalid.validate().is_err());
    invalid = base.clone();
    invalid.tiles.push(invalid.tiles[0].clone());
    assert!(invalid.validate().is_err());
    invalid = base.clone();
    invalid.tiles.last_mut().unwrap().value = 1;
    assert!(invalid.validate().is_err());
    invalid = base.clone();
    invalid.rack_size = 0;
    assert!(invalid.validate().is_err());
    invalid = base.clone();
    invalid
        .tiles
        .retain(|tile| tile.face == (TileFace::Blank {}));
    assert!(invalid.validate().is_err());
    invalid = base;
    invalid.scoreless_turn_limit = 0;
    assert!(invalid.validate().is_err());
}
