//! Independent #107 technical-enabler acceptance, derived from Atlas 1.25/1.31/1.39.
//! Authored before production on baseline b757166. No browser/hex-play/recovery claim.
use scrabble_game::{
    dictionary::{Dictionary, WordList},
    identity::{GameId, PlayerId},
    ruleset::{ContentIdentity, Premium, Ruleset, TileFace, english_fixture},
    state::GameState,
};
use scrabble_server::profiles::{
    BoardProfile, MatchRules, TileProfile, WordListProfile, compose_profiles,
};
use std::sync::Arc;

fn identity(name: &str) -> ContentIdentity {
    ContentIdentity {
        name: name.into(),
        revision: "1".into(),
    }
}
fn words(text: &str) -> WordListProfile {
    WordListProfile::new(WordList::from_text(identity("acceptance-words"), text).unwrap()).unwrap()
}
fn tiles() -> TileProfile {
    TileProfile::new(identity("acceptance-tiles"), english_fixture().tiles).unwrap()
}
fn board(size: u8) -> BoardProfile {
    BoardProfile::new(
        identity("acceptance-board"),
        size,
        vec![Premium::Normal; usize::from(size).pow(2)],
    )
    .unwrap()
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
fn independent_sources_compose_to_one_unchanged_engine_configuration() {
    let words = words("CAT\nAT\n");
    let tiles = tiles();
    let board = board(7);
    let result = compose_profiles(&words, &tiles, &board, settings()).unwrap();
    result.rules.validate().unwrap();
    assert_eq!(result.selection.word_list, *words.identity());
    assert_eq!(result.selection.tile, *tiles.identity());
    assert_eq!(result.selection.board, *board.identity());
    assert_eq!(result.rules.dictionary, *result.dictionary.identity());
    assert_eq!(result.rules.tiles, english_fixture().tiles);
    assert_eq!(result.rules.board_size, 7);
    assert_eq!(result.rules.premiums, vec![Premium::Normal; 49]);
    assert_eq!(result.rules.rack_size, english_fixture().rack_size);
    assert!(result.dictionary.contains("CAT"));
    assert!(!result.dictionary.contains("DOG"));
}

#[test]
fn normalized_word_content_is_deterministic_and_shared_without_copying_the_dictionary() {
    let first = words("CAT\nAT\nCAT\n");
    let reordered = words("AT\r\nCAT\r\n");
    let tiles = tiles();
    let board = board(7);
    let a = compose_profiles(&first, &tiles, &board, settings()).unwrap();
    let b = compose_profiles(&reordered, &tiles, &board, settings()).unwrap();
    let repeat = compose_profiles(&first, &tiles, &board, settings()).unwrap();
    assert_eq!(a.rules, b.rules);
    assert_eq!(a.rules, repeat.rules);
    assert_eq!(a.selection, b.selection);
    assert!(Arc::ptr_eq(&a.dictionary, &repeat.dictionary));
}

#[test]
fn changing_any_source_content_changes_match_identity_without_mutating_existing_inputs() {
    let original_words = words("CAT\nAT\n");
    let original_tiles = tiles();
    let original_board = board(7);
    let original = compose_profiles(
        &original_words,
        &original_tiles,
        &original_board,
        settings(),
    )
    .unwrap();
    let changed_words = words("CAT\nAT\nDOG\n");
    let mut definitions = english_fixture().tiles;
    definitions[0].value += 1;
    let changed_tiles = TileProfile::new(identity("acceptance-tiles"), definitions).unwrap();
    let mut premiums = vec![Premium::Normal; 49];
    premiums[24] = Premium::DoubleWord;
    let changed_board = BoardProfile::new(identity("acceptance-board"), 7, premiums).unwrap();
    for changed in [
        compose_profiles(&changed_words, &original_tiles, &original_board, settings()).unwrap(),
        compose_profiles(&original_words, &changed_tiles, &original_board, settings()).unwrap(),
        compose_profiles(&original_words, &original_tiles, &changed_board, settings()).unwrap(),
        compose_profiles(&original_words, &original_tiles, &board(9), settings()).unwrap(),
    ] {
        assert_ne!(changed.rules.identity, original.rules.identity);
    }
    assert!(!original.dictionary.contains("DOG"));
    assert_eq!(
        original.rules,
        compose_profiles(
            &original_words,
            &original_tiles,
            &original_board,
            settings()
        )
        .unwrap()
        .rules
    );
}

#[test]
fn each_advanced_setting_participates_in_the_immutable_match_identity() {
    let words = words("CAT\n");
    let tiles = tiles();
    let board = board(7);
    let original = compose_profiles(&words, &tiles, &board, settings()).unwrap();
    for index in 0..6 {
        let mut changed = settings();
        match index {
            0 => changed.rack_size = 6,
            1 => changed.bingo_bonus += 1,
            2 => changed.exchange_minimum_bag += 1,
            3 => changed.scoreless_turn_limit += 1,
            4 => changed.minimum_players = 3,
            5 => changed.maximum_players = 3,
            _ => unreachable!(),
        }
        let changed = compose_profiles(&words, &tiles, &board, changed).unwrap();
        assert_ne!(changed.rules.identity, original.rules.identity);
    }
}

#[test]
fn incompatible_dictionary_is_rejected_without_substitution() {
    let tiles = tiles();
    let board = board(7);
    let incompatible = words("ÖL\n");
    let error = compose_profiles(&incompatible, &tiles, &board, settings())
        .err()
        .expect("unsupported dictionary alphabet");
    assert!(!error.to_string().is_empty());
    let valid = compose_profiles(&words("CAT\n"), &tiles, &board, settings()).unwrap();
    assert!(valid.dictionary.contains("CAT"));
    assert!(!valid.dictionary.contains("ÖL"));
}

#[test]
fn supported_unicode_letters_use_the_selected_alphabet_without_language_substitution() {
    let words = words("ÖL\n");
    let mut definitions = english_fixture().tiles;
    definitions.push(scrabble_game::ruleset::TileDefinition {
        face: TileFace::Letter { letter: 'Ö' },
        count: 2,
        value: 4,
    });
    let tiles = TileProfile::new(identity("unicode-tiles"), definitions).unwrap();
    let composed = compose_profiles(&words, &tiles, &board(7), settings()).unwrap();
    assert!(composed.dictionary.contains("ÖL"));
    assert_eq!(composed.selection.tile, *tiles.identity());
    assert!(
        composed
            .rules
            .tiles
            .iter()
            .any(|tile| tile.face == TileFace::Letter { letter: 'Ö' })
    );
}

#[test]
fn selected_source_identities_are_part_of_match_identity_even_when_content_is_equal() {
    let words = words("CAT\n");
    let tiles = tiles();
    let board = board(7);
    let original = compose_profiles(&words, &tiles, &board, settings()).unwrap();
    let renamed_words =
        WordListProfile::new(WordList::from_text(identity("other-word-list"), "CAT\n").unwrap())
            .unwrap();
    let renamed_tiles = TileProfile::new(identity("other-tiles"), english_fixture().tiles).unwrap();
    let renamed_board =
        BoardProfile::new(identity("other-board"), 7, vec![Premium::Normal; 49]).unwrap();
    for changed in [
        compose_profiles(&renamed_words, &tiles, &board, settings()).unwrap(),
        compose_profiles(&words, &renamed_tiles, &board, settings()).unwrap(),
        compose_profiles(&words, &tiles, &renamed_board, settings()).unwrap(),
    ] {
        assert_ne!(changed.rules.identity, original.rules.identity);
    }
}

#[test]
fn valid_source_profiles_can_still_fail_combined_rack_and_player_requirements() {
    let definitions = vec![scrabble_game::ruleset::TileDefinition {
        face: TileFace::Letter { letter: 'A' },
        count: 12,
        value: 1,
    }];
    let tiles = TileProfile::new(identity("small-bag"), definitions).unwrap();
    let words = words("A\n");
    let board = board(7);
    let mut small_match = settings();
    small_match.rack_size = 3;
    small_match.maximum_players = 4;
    assert!(compose_profiles(&words, &tiles, &board, small_match).is_ok());
    assert!(compose_profiles(&words, &tiles, &board, settings()).is_err());
}

#[test]
fn invalid_profiles_and_combined_bounds_fail_closed() {
    assert!(BoardProfile::new(identity("bad-board"), 8, vec![Premium::Normal; 64]).is_err());
    assert!(BoardProfile::new(identity("bad-board"), 7, vec![Premium::Normal; 48]).is_err());
    assert!(BoardProfile::new(identity("bad name"), 7, vec![Premium::Normal; 49]).is_err());
    let mut duplicate = english_fixture().tiles;
    duplicate.push(duplicate[0].clone());
    assert!(TileProfile::new(identity("duplicate"), duplicate).is_err());
    let mut bad_blank = english_fixture().tiles;
    bad_blank
        .iter_mut()
        .find(|tile| tile.face == TileFace::Blank {})
        .unwrap()
        .value = 1;
    assert!(TileProfile::new(identity("bad-blank"), bad_blank).is_err());
    let words = words("CAT\n");
    let tiles = tiles();
    let board = board(7);
    for index in 0..4 {
        let mut invalid = settings();
        match index {
            0 => invalid.rack_size = 0,
            1 => invalid.scoreless_turn_limit = 0,
            2 => invalid.minimum_players = 1,
            3 => invalid.maximum_players = 5,
            _ => unreachable!(),
        }
        assert!(compose_profiles(&words, &tiles, &board, invalid).is_err());
    }
}

#[test]
fn composed_rules_roundtrip_into_deterministic_actual_game_state_and_public_configuration() {
    let composed = compose_profiles(&words("CAT\nAT\n"), &tiles(), &board(7), settings()).unwrap();
    let serialized = serde_json::to_vec(&composed.rules).unwrap();
    let recovered_rules: Ruleset = serde_json::from_slice(&serialized).unwrap();
    let mut original = GameState::new(GameId::new(107), composed.rules.clone(), [7; 32]).unwrap();
    let mut reconstructed = GameState::new(GameId::new(107), recovered_rules, [7; 32]).unwrap();
    for state in [&mut original, &mut reconstructed] {
        state.add_player(PlayerId::new(1), "First".into()).unwrap();
        state.add_player(PlayerId::new(2), "Second".into()).unwrap();
        state.deal_initial_racks().unwrap();
        state.verify_tile_conservation().unwrap();
    }
    assert_eq!(
        original.canonical_bytes().unwrap(),
        reconstructed.canonical_bytes().unwrap()
    );
    assert_eq!(original.ruleset(), &composed.rules);
    let public = serde_json::to_value(original.public_snapshot()).unwrap();
    assert_eq!(
        public["configuration"],
        serde_json::to_value(&composed.rules).unwrap()
    );
    assert!(public.get("words").is_none());
}
