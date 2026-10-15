//! Independent acceptance for multi-letter tile faces at the server composition boundary
//! (#116, Atlas 1.21/1.25). Specification: docs/specs/multi-letter-tiles.md.
//! Authored before production edits on baseline 2473b12.
use game_server::{GameSimulation, MatchRuntime, ReconnectToken};
use scrabble_game::{
    dictionary::{Dictionary, WordList},
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{
        Command, CommandEnvelope, MAX_HISTORY_TURNS, Placement, PlayerSnapshot, PublicTurn,
        ScoreChange, TurnAction,
    },
    ruleset::{ContentIdentity, Premium, Ruleset, TileDefinition, TileFace, english_fixture},
    state::GameState,
};
use scrabble_server::{
    config::DictionaryConfig,
    dictionary,
    profiles::{
        BoardProfile, MatchRules, ProfileError, TileProfile, WordListProfile, compose_profiles,
    },
    simulation::ScrabbleSimulation,
};
use serde_json::{Value, json};
use std::fs;

fn identity(name: &str) -> ContentIdentity {
    ContentIdentity {
        name: name.into(),
        revision: "1".into(),
    }
}
fn face(text: &str) -> TileFace {
    let mut chars = text.chars();
    match (text, chars.next(), chars.next()) {
        ("?", _, _) => TileFace::Blank {},
        (_, Some(letter), None) => TileFace::Letter { letter },
        _ => TileFace::Letters {
            letters: text.into(),
        },
    }
}
fn definition(text: &str, count: u16, value: u16) -> TileDefinition {
    TileDefinition {
        face: face(text),
        count,
        value,
    }
}
/// CH, A, T and blanks: C and H exist only on the CH face.
fn ch_tiles() -> Vec<TileDefinition> {
    vec![
        definition("CH", 10, 5),
        definition("A", 20, 1),
        definition("T", 20, 1),
        definition("?", 2, 0),
    ]
}
fn words(text: &str) -> WordListProfile {
    WordListProfile::new(WordList::from_text(identity("multi-letter-words"), text).unwrap())
        .unwrap()
}
fn board() -> BoardProfile {
    BoardProfile::new(identity("multi-letter-board"), 7, vec![Premium::Normal; 49]).unwrap()
}
fn settings() -> MatchRules {
    let rules = english_fixture();
    MatchRules {
        rack_size: rules.rack_size,
        bingo_bonus: rules.bingo_bonus,
        exchange_minimum_bag: rules.exchange_minimum_bag,
        scoreless_turn_limit: rules.scoreless_turn_limit,
        minimum_players: rules.minimum_players,
        maximum_players: rules.maximum_players,
    }
}

#[test]
fn tile_profiles_keep_multi_letter_faces_and_check_word_lists_by_face_letters() {
    let tiles = TileProfile::new(identity("multi-letter-tiles"), ch_tiles()).unwrap();
    // C and H are supplied by the CH face: nothing is dropped from the alphabet.
    let composed =
        compose_profiles(&words("CHAT\nAT\nTA\n"), &tiles, &board(), settings()).unwrap();
    composed.rules.validate().unwrap();
    assert_eq!(composed.rules.tiles, ch_tiles(), "faces kept verbatim");
    assert!(
        serde_json::to_string(&composed.rules)
            .unwrap()
            .contains(r#"{"kind":"letters","letters":"CH"}"#)
    );
    assert!(composed.dictionary.contains("CHAT"));
    GameState::new(GameId::new(1), composed.rules, [1; 32]).unwrap();

    // Letters on no face are still reported, by letter, without substitution.
    assert_eq!(
        compose_profiles(&words("CHAT\nDOG\n"), &tiles, &board(), settings()).err(),
        Some(ProfileError::UnsupportedLetters {
            letters: vec!['D', 'G', 'O'],
        })
    );

    // The tile-profile identity covers the whole face.
    let mut changed = ch_tiles();
    changed[0] = definition("HC", 10, 5);
    assert_ne!(
        TileProfile::new(identity("multi-letter-tiles"), changed)
            .unwrap()
            .identity(),
        tiles.identity()
    );

    // Invalid faces fail tile-profile validation.
    for letters in ["C", "Ch", "C1", "SCHTZ"] {
        let mut invalid = ch_tiles();
        invalid.push(TileDefinition {
            face: TileFace::Letters {
                letters: letters.into(),
            },
            count: 1,
            value: 1,
        });
        assert!(
            matches!(
                TileProfile::new(identity("multi-letter-tiles"), invalid),
                Err(ProfileError::InvalidTiles(_))
            ),
            "{letters:?}"
        );
    }
    let mut duplicate = ch_tiles();
    duplicate.push(definition("CH", 1, 5));
    assert!(matches!(
        TileProfile::new(identity("multi-letter-tiles"), duplicate),
        Err(ProfileError::InvalidTiles(_))
    ));
}

#[test]
fn deployment_loader_accepts_words_spelled_with_multi_letter_faces() {
    let directory = tempfile::tempdir().unwrap();
    let mut rules = english_fixture();
    rules.identity = identity("multi-letter-deployment");
    rules.dictionary = identity("multi-letter-words");
    rules.tiles = ch_tiles();
    rules.validate().unwrap();
    fs::write(
        directory.path().join("rules.json"),
        serde_json::to_vec(&rules).unwrap(),
    )
    .unwrap();
    let config = DictionaryConfig {
        file: directory.path().join("words.txt"),
        name: "multi-letter-words".into(),
        revision: "1".into(),
    };

    fs::write(&config.file, "CHAT\nAT\nTA\n").unwrap();
    let (loaded, words) =
        dictionary::load(Some(&directory.path().join("rules.json")), Some(&config)).unwrap();
    assert_eq!(loaded.tiles, ch_tiles());
    assert!(words.contains("CHAT"));

    fs::write(&config.file, "CHAT\nDOG\n").unwrap();
    assert!(dictionary::load(Some(&directory.path().join("rules.json")), Some(&config)).is_err());
}

/// 64 distinct two-letter faces of exactly four UTF-8 bytes (two 2-byte uppercase letters).
fn widest_faces() -> Vec<String> {
    let letters: Vec<char> = ('\u{c0}'..='\u{de}')
        .filter(|letter| letter.is_uppercase())
        .collect();
    let faces: Vec<String> = letters
        .iter()
        .flat_map(|first| letters.iter().map(move |second| format!("{first}{second}")))
        .take(64)
        .collect();
    assert!(faces.iter().all(|face| face.len() == 4));
    faces
}
fn widest_rules() -> Ruleset {
    let mut rules = english_fixture();
    rules.identity.name = "n".repeat(64);
    rules.identity.revision = "v".repeat(192);
    rules.dictionary = rules.identity.clone();
    rules.tiles = widest_faces()
        .into_iter()
        .map(|letters| TileDefinition {
            face: TileFace::Letters { letters },
            count: 3,
            value: u16::MAX,
        })
        .collect();
    // Today's rules still require one single-letter face (spec open question 2).
    rules.tiles[0].face = TileFace::Letter {
        letter: '\u{10400}',
    };
    rules.tiles[0].count = 11;
    rules.rack_size = 15;
    rules.premiums = vec![Premium::TripleLetter; 225];
    rules.validate().unwrap();
    rules
}
fn widest_history() -> Vec<PublicTurn> {
    (0..MAX_HISTORY_TURNS)
        .map(|index| PublicTurn {
            turn: TurnId::new(u64::MAX - index as u64),
            player_id: PlayerId::new(u64::MAX),
            action: TurnAction::Commit {
                // 15 cells of four-byte faces.
                words: vec!["ÀÁ".repeat(15); 16],
                move_score: u32::MAX,
                blank_count: 15,
            },
            scores: (0..4)
                .map(|index| ScoreChange {
                    player_id: PlayerId::new(u64::MAX - index),
                    delta: -4294967295,
                    score: i32::MIN,
                })
                .collect(),
        })
        .collect()
}

/// Mirrors the single-letter worst cases in `history.rs`: four-byte multi-letter faces in
/// the configuration, the private rack, board cells and history words stay inside the
/// native snapshot limit. Synthetic sizes deliberately overestimate simultaneous legal play.
#[test]
fn largest_multi_letter_public_snapshot_and_checkpoint_stay_inside_native_limit() {
    let mut runtime = MatchRuntime::new_with_replay_capture(
        ScrabbleSimulation::new(GameId::new(1), english_fixture(), [7; 32]).unwrap(),
        100,
    );
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
    let mut snapshot: Value =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    let rules = widest_rules();
    let public = &mut snapshot["public"];
    public["board"] = (0..225_u8)
        .map(|index| {
            json!({
                "coordinate": {"row": index / 15, "column": index % 15},
                "letter": "ÀÁ",
                "is_blank": true,
            })
        })
        .collect();
    public["players"] = (0..4_u64)
        .map(|index| {
            json!({
                "id": (u64::MAX - index).to_string(),
                "display_name": "\u{10400}".repeat(64),
                "score": i32::MIN,
                "rack_count": 15,
                "connected": false,
            })
        })
        .collect();
    public["history"] = serde_json::to_value(widest_history()).unwrap();
    public["configuration"] = serde_json::to_value(&rules).unwrap();
    public["ruleset"] = serde_json::to_value(&rules.identity).unwrap();
    public["dictionary"] = serde_json::to_value(&rules.dictionary).unwrap();
    snapshot["own_rack"]["tiles"] = (0..15)
        .map(|_| json!({"id": u64::MAX.to_string(), "face": {"kind": "letters", "letters": "ÀÁ"}}))
        .collect();
    // The DTOs accept multi-letter board letters and rack faces.
    let typed: PlayerSnapshot = serde_json::from_value(snapshot).unwrap();
    let bytes = serde_json::to_vec(&typed).unwrap();
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

    // Private checkpoint: 200 committed multi-letter tiles and maximal history.
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
    let mut game: Value = serde_json::from_slice(&state.canonical_bytes().unwrap()).unwrap();
    let committed = game["board"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| !entry.is_null())
        .unwrap()
        .clone();
    let mut board = vec![committed; 200];
    board.extend(vec![Value::Null; 25]);
    game["board"] = json!(board);
    game["bag"] = json!([]);
    for player in game["players"].as_array_mut().unwrap() {
        player["id"] = json!(u32::MAX.to_string());
        player["rack"] = json!([]);
        player["score"] = json!(i32::MIN);
    }
    game["history"] = serde_json::to_value(widest_history()).unwrap();
    game["revision"] = json!(u64::MAX.to_string());
    game["phase"]["turn"] = json!(u64::MAX.to_string());
    game["phase"]["active_player"] = json!(u32::MAX.to_string());
    let simulation = ScrabbleSimulation::new(GameId::new(u64::MAX), rules, [7; 32]).unwrap();
    let mut envelope: Value =
        serde_json::from_slice(&simulation.snapshot().unwrap().payload).unwrap();
    envelope["game"] = game;
    envelope["admitted"] = json!([u32::MAX - 3, u32::MAX - 2, u32::MAX - 1, u32::MAX]);
    envelope["last_admitted_player"] = json!(u32::MAX);
    envelope["lifecycle"] = json!({"created_at": u64::MAX, "finished_at_tick": u64::MAX});
    envelope["board"] = json!({"owner": u32::MAX});
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
