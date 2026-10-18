//! Independent acceptance contract for multi-letter tile faces (#116, enabler of #98).
//! Specification: docs/specs/multi-letter-tiles.md (Atlas 1.2, 1.21, 1.23, 1.25).
//! Authored before production edits on baseline 2473b12, using only the public engine API.
//!
//! A multi-letter face (`TileFace::Letters { letters }`) is one physical tile: one rack
//! slot, one board cell and one configured value. It contributes its whole face, in order,
//! to every spelled word.
use scrabble_game::{
    commit::{self, CommitError},
    dictionary::{self, Dictionary, DictionaryError, WordList},
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    placement::{Direction, PlacementError, validate},
    protocol::{Command, CommandEnvelope, Phase, Placement, TurnAction},
    ruleset::{
        BoardShape, ContentIdentity, Premium, PremiumPolicy, Ruleset, TileDefinition, TileFace,
        WordLengthMetric, english_fixture, german_fixture, validate_tiles,
    },
    scoring::{MoveScore, calculate},
    session::{CommandError, GameSession},
    state::{GameState, StateError},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

const P1: PlayerId = PlayerId::new(1);
const P2: PlayerId = PlayerId::new(2);
/// CH is worth 5 points: once per tile, never once per letter.
const CH_VALUE: u16 = 5;
const SCH_VALUE: u16 = 8;
/// Square fixture: 20 CH + 40 A + 30 T + 2 blanks.
const SQUARE_TILES: usize = 92;
const WORDS: &[&str] = &["CHAT", "CHA", "ACH", "TA", "AT", "AA"];
/// Opening through the double-word center: CH A T spells CHAT over three cells.
const OPENING: [(u8, u8, &str); 3] = [(7, 7, "CH"), (7, 8, "A"), (7, 9, "T")];
/// Reply below the opening: main word CHA, cross words ACH (through the new CH on the
/// triple-letter cell) and TA.
const REPLY: [(u8, u8, &str); 2] = [(8, 8, "CH"), (8, 9, "A")];

/// `"?"` is a blank, a one-character string a single letter, anything else a multi-letter face.
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
fn coordinate(row: u8, column: u8) -> Coordinate {
    Coordinate::new(row, column).unwrap()
}
fn words_identity() -> ContentIdentity {
    ContentIdentity {
        name: "multi-letter-acceptance-words".into(),
        revision: "1".into(),
    }
}
fn dictionary(words: &[&str]) -> WordList {
    WordList::from_text(words_identity(), &words.join("\n")).unwrap()
}
/// 15 × 15 square board: double word at the center (7, 7), triple letter at (8, 8).
fn square_rules(rack_size: u8) -> Ruleset {
    let mut rules = english_fixture();
    rules.identity.name = "multi-letter-acceptance".into();
    rules.dictionary = words_identity();
    rules.premiums = vec![Premium::Normal; 225];
    rules.premiums[7 * 15 + 7] = Premium::DoubleWord;
    rules.premiums[8 * 15 + 8] = Premium::TripleLetter;
    rules.rack_size = rack_size;
    rules.tiles = vec![
        definition("CH", 20, CH_VALUE),
        definition("A", 40, 1),
        definition("T", 30, 1),
        definition("?", 2, 0),
    ];
    rules
}
fn holds(state: &GameState, player: PlayerId, text: &str) -> usize {
    state
        .rack(player)
        .unwrap()
        .iter()
        .filter(|tile| tile.face() == face(text))
        .count()
}
fn has_all(state: &GameState, player: PlayerId, needs: &[(&str, usize)]) -> bool {
    needs
        .iter()
        .all(|&(text, count)| holds(state, player, text) >= count)
}
fn dealt(rules: &Ruleset, seed: u8) -> GameState {
    let mut state = GameState::new(GameId::new(1), rules.clone(), [seed; 32]).unwrap();
    state.add_player(P1, "Ada".into()).unwrap();
    state.add_player(P2, "Lin".into()).unwrap();
    state.deal_initial_racks().unwrap();
    state
}
/// Deterministically selects the first seed whose dealt racks hold the requested faces.
fn seed_for(rules: &Ruleset, first: &[(&str, usize)], second: &[(&str, usize)]) -> u8 {
    (0..=255)
        .find(|&seed| {
            let state = dealt(rules, seed);
            has_all(&state, P1, first) && has_all(&state, P2, second)
        })
        .expect("deterministic multi-letter rack fixture")
}
fn game(rules: &Ruleset, first: &[(&str, usize)], second: &[(&str, usize)]) -> GameState {
    dealt(rules, seed_for(rules, first, second))
}
fn tiles(state: &GameState, player: PlayerId, cells: &[(u8, u8, &str)]) -> Vec<Placement> {
    let rack = state.rack(player).unwrap();
    let mut used = BTreeSet::new();
    cells
        .iter()
        .map(|&(row, column, text)| {
            let tile = rack
                .iter()
                .find(|tile| tile.face() == face(text) && !used.contains(&tile.id()))
                .expect("fixture rack holds the requested face");
            used.insert(tile.id());
            Placement {
                tile_id: tile.id(),
                coordinate: coordinate(row, column),
                blank_as: None,
            }
        })
        .collect()
}
fn texts(score: &MoveScore) -> Vec<&str> {
    score.words().iter().map(|word| word.text()).collect()
}
fn directions(score: &MoveScore) -> Vec<Direction> {
    score.words().iter().map(|word| word.direction()).collect()
}
fn last_action(state: &GameState) -> TurnAction {
    state
        .public_snapshot()
        .history
        .last()
        .unwrap()
        .action
        .clone()
}
/// `(row, column, letter, is_blank)` of every public board tile, read from the wire JSON.
fn wire_board(snapshot: &Value) -> Vec<(u64, u64, String, bool)> {
    let mut board: Vec<_> = snapshot["board"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tile| {
            (
                tile["coordinate"]["row"].as_u64().unwrap(),
                tile["coordinate"]["column"].as_u64().unwrap(),
                tile["letter"].as_str().unwrap().to_owned(),
                tile["is_blank"].as_bool().unwrap(),
            )
        })
        .collect();
    board.sort();
    board
}
fn opened(state: &mut GameState) {
    let placements = tiles(state, P1, &OPENING);
    commit::apply_with_dictionary(state, P1, &placements, &dictionary(WORDS)).unwrap();
}

#[test]
fn ch_a_t_spells_chat_across_three_cells() {
    let rules = square_rules(7);
    let mut state = game(&rules, &[("CH", 1), ("A", 1), ("T", 1)], &[]);
    assert_eq!(state.remaining_tiles(), SQUARE_TILES - 14);
    let placements = tiles(&state, P1, &OPENING);

    let validated = validate(&state, P1, &placements).unwrap();
    assert_eq!(validated.words().len(), 1);
    let word = &validated.words()[0];
    assert_eq!(word.direction(), Direction::Horizontal);
    assert_eq!(word.text(), "CHAT");
    assert_eq!(word.tiles().len(), 3, "one cell per physical tile");
    assert!(word.tiles().iter().all(|tile| tile.is_new()));
    assert_eq!(
        word.tiles()
            .iter()
            .map(|tile| (tile.coordinate().row(), tile.coordinate().column()))
            .collect::<Vec<_>>(),
        [(7, 7), (7, 8), (7, 9)]
    );

    // (CH 5 + A 1 + T 1) × double word = 14.
    let preview = calculate(&validated).unwrap();
    assert_eq!(texts(&preview), ["CHAT"]);
    assert_eq!(preview.word_scores(), [14]);
    assert_eq!(preview.bingo_bonus(), 0);
    assert_eq!(preview.total(), 14);

    let committed =
        commit::apply_with_dictionary(&mut state, P1, &placements, &dictionary(WORDS)).unwrap();
    assert_eq!(committed, preview);
    assert_eq!(
        state.board_tile(coordinate(7, 7)).unwrap().tile().face(),
        face("CH")
    );
    assert_eq!(
        state.board_tile(coordinate(7, 7)).unwrap().tile().value(),
        CH_VALUE
    );
    assert!(state.board_tile(coordinate(7, 10)).is_none());
    assert_eq!(
        last_action(&state),
        TurnAction::Commit {
            words: vec!["CHAT".into()],
            move_score: 14,
            blank_count: 0,
        }
    );
    let snapshot = state.public_snapshot();
    assert_eq!(snapshot.players[0].score, 14);
    // Three physical tiles left the rack and three were drawn.
    assert_eq!(snapshot.players[0].rack_count, 7);
    assert_eq!(state.rack(P1).unwrap().len(), 7);
    assert_eq!(state.remaining_tiles(), SQUARE_TILES - 14 - 3);
    assert_eq!(usize::from(snapshot.remaining_tiles), SQUARE_TILES - 17);
    assert_eq!(
        wire_board(&serde_json::to_value(&snapshot).unwrap()),
        [
            (7, 7, "CH".to_owned(), false),
            (7, 8, "A".to_owned(), false),
            (7, 9, "T".to_owned(), false),
        ]
    );
    state.verify_tile_conservation().unwrap();
}

#[test]
fn cross_words_through_a_multi_letter_tile_score_its_value_once_with_premiums() {
    let rules = square_rules(7);
    let mut state = game(
        &rules,
        &[("CH", 1), ("A", 2), ("T", 1)],
        &[("CH", 1), ("A", 1)],
    );
    opened(&mut state);

    // The new CH sits on the triple letter at (8, 8):
    //   main CHA = CH 5·3 + A 1 = 16, cross ACH = A 1 + CH 5·3 = 16, cross TA = 1 + 1 = 2.
    // Counting CH per letter, or applying its premium per letter, would change every number.
    let reply = tiles(&state, P2, &REPLY);
    let validated = validate(&state, P2, &reply).unwrap();
    assert_eq!(
        validated
            .words()
            .iter()
            .map(|word| (word.text(), word.tiles().len()))
            .collect::<Vec<_>>(),
        [
            ("CHA".to_owned(), 2),
            ("ACH".to_owned(), 2),
            ("TA".to_owned(), 2)
        ]
    );
    let preview = calculate(&validated).unwrap();
    assert_eq!(texts(&preview), ["CHA", "ACH", "TA"]);
    assert_eq!(
        directions(&preview),
        [
            Direction::Horizontal,
            Direction::Vertical,
            Direction::Vertical
        ]
    );
    assert_eq!(preview.word_scores(), [16, 16, 2]);
    assert_eq!(preview.total(), 34);
    let committed =
        commit::apply_with_dictionary(&mut state, P2, &reply, &dictionary(WORDS)).unwrap();
    assert_eq!(committed, preview);
    assert_eq!(state.public_snapshot().players[1].score, 34);
    assert_eq!(
        last_action(&state),
        TurnAction::Commit {
            words: vec!["CHA".into(), "ACH".into(), "TA".into()],
            move_score: 34,
            blank_count: 0,
        }
    );

    // A single A above the committed center CH forms ACH through it. The committed tile
    // keeps its value once and no longer earns the center's double word: 1 + 5 = 6.
    let single = tiles(&state, P1, &[(6, 7, "A")]);
    let score = calculate(&validate(&state, P1, &single).unwrap()).unwrap();
    assert_eq!(texts(&score), ["ACH"]);
    assert_eq!(directions(&score), [Direction::Vertical]);
    assert_eq!(score.total(), 1 + u32::from(CH_VALUE));
    commit::apply_with_dictionary(&mut state, P1, &single, &dictionary(WORDS)).unwrap();
    assert_eq!(state.public_snapshot().players[0].score, 14 + 6);
    state.verify_tile_conservation().unwrap();
}

#[test]
fn dictionary_validates_the_fully_spelled_words() {
    let rules = square_rules(7);
    let mut state = game(
        &rules,
        &[("CH", 1), ("A", 1), ("T", 1)],
        &[("CH", 1), ("A", 1)],
    );
    let opening = tiles(&state, P1, &OPENING);

    // Neither the first letter nor single-letter spellings stand in for the CH face.
    let partial = dictionary(&["CAT", "HAT", "CT", "AT", "TA", "CHA", "ACH"]);
    let invalid = DictionaryError::InvalidWords {
        words: vec!["CHAT".into()],
    };
    assert_eq!(
        dictionary::validate(&validate(&state, P1, &opening).unwrap(), &partial),
        Err(invalid.clone())
    );
    let before = state.canonical_bytes().unwrap();
    assert_eq!(
        commit::apply_with_dictionary(&mut state, P1, &opening, &partial),
        Err(CommitError::Dictionary(invalid))
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
    let only_chat = dictionary(&["CHAT"]);
    assert_eq!(
        dictionary::validate(&validate(&state, P1, &opening).unwrap(), &only_chat),
        Ok(())
    );
    commit::apply_with_dictionary(&mut state, P1, &opening, &only_chat).unwrap();

    // Every spelled word of the reply is checked, including the cross word through CH.
    let reply = tiles(&state, P2, &REPLY);
    for (words, missing) in [
        (&["CHA", "TA"][..], vec!["ACH"]),
        (&["TA"][..], vec!["ACH", "CHA"]),
        (&["CHA", "ACH"][..], vec!["TA"]),
    ] {
        let before = state.canonical_bytes().unwrap();
        assert_eq!(
            commit::apply_with_dictionary(&mut state, P2, &reply, &dictionary(words)),
            Err(CommitError::Dictionary(DictionaryError::InvalidWords {
                words: missing.into_iter().map(String::from).collect(),
            }))
        );
        assert_eq!(state.canonical_bytes().unwrap(), before);
    }
    commit::apply_with_dictionary(&mut state, P2, &reply, &dictionary(&["CHA", "ACH", "TA"]))
        .unwrap();
    assert_eq!(state.public_snapshot().players[1].score, 34);
}

#[test]
fn final_rack_deductions_count_each_tile_value_once() {
    let mut rules = square_rules(7);
    rules.scoreless_turn_limit = 1;
    let state = game(&rules, &[("CH", 2)], &[]);
    let rack_value = |player: PlayerId| -> i32 {
        state
            .rack(player)
            .unwrap()
            .iter()
            .map(|tile| match tile.face() {
                TileFace::Letters { letters } if letters == "CH" => i32::from(CH_VALUE),
                TileFace::Blank {} => 0,
                _ => 1,
            })
            .sum()
    };
    let expected = [-rack_value(P1), -rack_value(P2)];
    assert!(expected[0] <= -2 * i32::from(CH_VALUE));
    let mut session = GameSession::new(state.clone());
    session
        .apply(P1, 1, &envelope(P1, 1, 0, Command::Pass {}))
        .unwrap();
    let snapshot = session.public_snapshot();
    assert!(matches!(snapshot.phase, Phase::Finished { .. }));
    assert_eq!(
        snapshot
            .players
            .iter()
            .map(|player| player.score)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn racks_refills_and_bingo_count_physical_tiles() {
    // CH A T is three tiles but four letters. With three-tile racks it empties the rack and
    // earns the bingo bonus; with four-tile racks it does not.
    for (rack_size, bingo) in [(3_u8, 50_u32), (4, 0)] {
        let rules = square_rules(rack_size);
        let mut state = game(&rules, &[("CH", 1), ("A", 1), ("T", 1)], &[]);
        let dealt = 2 * usize::from(rack_size);
        assert_eq!(state.remaining_tiles(), SQUARE_TILES - dealt);
        for (index, player) in [P1, P2].into_iter().enumerate() {
            assert_eq!(state.rack(player).unwrap().len(), usize::from(rack_size));
            assert_eq!(state.public_snapshot().players[index].rack_count, rack_size);
        }
        let placements = tiles(&state, P1, &OPENING);
        let score =
            commit::apply_with_dictionary(&mut state, P1, &placements, &dictionary(WORDS)).unwrap();
        assert_eq!(score.word_scores(), [14], "rack size {rack_size}");
        assert_eq!(score.bingo_bonus(), bingo, "rack size {rack_size}");
        assert_eq!(score.total(), 14 + bingo, "rack size {rack_size}");
        assert_eq!(state.rack(P1).unwrap().len(), usize::from(rack_size));
        assert_eq!(state.remaining_tiles(), SQUARE_TILES - dealt - 3);
        state.verify_tile_conservation().unwrap();
    }
}

fn envelope(player: PlayerId, sequence: u32, turn: u64, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: player,
        sequence,
        expected_turn: TurnId::new(turn),
        command,
    }
}

#[test]
fn exchange_counts_physical_bag_tiles() {
    // 7 tiles, two-tile racks for two players: three tiles stay in the bag. They spell at
    // least five letters, but the exchange threshold counts tiles.
    for (minimum, allowed) in [(4_u16, false), (3, true)] {
        let mut rules = square_rules(2);
        rules.maximum_players = 2;
        rules.exchange_minimum_bag = minimum;
        rules.tiles = vec![definition("CH", 6, CH_VALUE), definition("A", 1, 1)];
        let mut session = GameSession::new(GameState::new(GameId::new(1), rules, [3; 32]).unwrap());
        session.add_player(P1, "Ada".into()).unwrap();
        session.add_player(P2, "Lin".into()).unwrap();
        session
            .apply(P1, 1, &envelope(P1, 1, 0, Command::Start {}))
            .unwrap();
        assert_eq!(session.state().remaining_tiles(), 3);
        let selected = session
            .state()
            .rack(P1)
            .unwrap()
            .iter()
            .find(|tile| tile.face() == face("CH"))
            .expect("six of seven tiles are CH")
            .id();
        let command = envelope(
            P1,
            2,
            0,
            Command::Exchange {
                tile_ids: vec![selected],
            },
        );
        let before = session.state().canonical_bytes().unwrap();
        let result = session.apply(P1, 2, &command);
        if allowed {
            result.unwrap();
            let rack = session.state().rack(P1).unwrap();
            assert_eq!(rack.len(), 2);
            assert!(rack.iter().all(|tile| tile.id() != selected));
            assert_eq!(session.state().remaining_tiles(), 3);
            assert_eq!(session.public_snapshot().players[0].rack_count, 2);
            assert_eq!(
                last_action(session.state()),
                TurnAction::Exchange { tile_count: 1 }
            );
        } else {
            assert_eq!(
                result,
                Err(CommandError::State(StateError::ExchangeUnavailable))
            );
            assert_eq!(session.state().canonical_bytes().unwrap(), before);
        }
        session.state().verify_tile_conservation().unwrap();
    }
}

#[test]
fn invalid_multi_letter_face_configurations_are_rejected() {
    let base = || vec![definition("A", 40, 1), definition("T", 30, 1)];
    let with = |extra: Vec<TileDefinition>| {
        let mut tiles = base();
        tiles.extend(extra);
        tiles
    };
    let mut cases = Vec::new();
    for letters in [
        "", "C", "Ä", "Ch", "cH", "ch", "C1", "C H", "C-", " CH", "SCHTZ", "ÄÖÜ",
    ] {
        cases.push((
            format!("face {letters:?}"),
            with(vec![TileDefinition {
                face: TileFace::Letters {
                    letters: letters.into(),
                },
                count: 2,
                value: 4,
            }]),
        ));
    }
    cases.push((
        "duplicate face".into(),
        with(vec![definition("CH", 2, 4), definition("CH", 1, 4)]),
    ));
    cases.push(("empty count".into(), with(vec![definition("CH", 0, 4)])));
    for (label, tiles) in cases {
        assert!(validate_tiles(&tiles).is_err(), "{label}");
        let mut rules = square_rules(7);
        rules.tiles = tiles;
        assert!(rules.validate().is_err(), "{label}");
        assert!(
            matches!(
                GameState::new(GameId::new(1), rules, [1; 32]),
                Err(StateError::InvalidRuleset(_))
            ),
            "{label}"
        );
    }

    // Two to four letters within four UTF-8 bytes, alongside single letters they share.
    let valid = with(
        ["C", "H", "S", "CH", "SCH", "TSCH", "ÄU", "ÄÖ", "DŽ"]
            .into_iter()
            .map(|text| definition(text, 2, 3))
            .chain([definition("?", 2, 0)])
            .collect(),
    );
    assert_eq!(validate_tiles(&valid), Ok(70 + 9 * 2 + 2));
    let mut rules = square_rules(7);
    rules.tiles = valid;
    rules.validate().unwrap();
    GameState::new(GameId::new(1), rules, [1; 32]).unwrap();

    // Wire form of the variant; unknown fields and malformed faces are rejected.
    let parsed: TileDefinition = serde_json::from_value(
        json!({"face": {"kind": "letters", "letters": "CH"}, "count": 2, "value": 5}),
    )
    .unwrap();
    assert_eq!(parsed, definition("CH", 2, 5));
    assert_eq!(
        serde_json::to_value(definition("CH", 2, 5)).unwrap(),
        json!({"face": {"kind": "letters", "letters": "CH"}, "count": 2, "value": 5})
    );
    for invalid in [
        json!({"kind": "letters", "letters": "CH", "extra": true}),
        json!({"kind": "letters"}),
        json!({"kind": "letter", "letter": "CH"}),
        json!({"kind": "digraph", "letters": "CH"}),
    ] {
        assert!(
            serde_json::from_value::<TileFace>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn blanks_keep_single_letter_assignments() {
    let mut rules = square_rules(7);
    rules.tiles[3] = definition("?", 10, 0);
    let state = game(&rules, &[("?", 1), ("A", 1)], &[]);
    let rack = state.rack(P1).unwrap();
    let blank = rack
        .iter()
        .find(|tile| tile.face() == TileFace::Blank {})
        .unwrap()
        .id();
    let a = tiles(&state, P1, &[(7, 8, "A")]).remove(0);
    let opening = |assigned: char| {
        vec![
            Placement {
                tile_id: blank,
                coordinate: coordinate(7, 7),
                blank_as: Some(assigned),
            },
            a.clone(),
        ]
    };
    // A blank can only become a configured single letter, never a letter that exists only
    // inside the CH face.
    for assigned in ['C', 'H'] {
        assert_eq!(
            validate(&state, P1, &opening(assigned)).err(),
            Some(PlacementError::Storage(StateError::InvalidBlankAssignment)),
            "{assigned}"
        );
    }
    let validated = validate(&state, P1, &opening('T')).unwrap();
    assert_eq!(validated.words()[0].text(), "TA");

    // A multi-letter blank assignment cannot even be decoded.
    let command = envelope(
        P1,
        2,
        0,
        Command::Commit {
            placements: opening('A'),
        },
    );
    let mut wire = serde_json::to_value(&command).unwrap();
    assert!(CommandEnvelope::decode(&serde_json::to_vec(&wire).unwrap()).is_ok());
    wire["command"]["placements"][0]["blank_as"] = json!("CH");
    assert!(CommandEnvelope::decode(&serde_json::to_vec(&wire).unwrap()).is_err());
}

/// Hexagon of radius 3 (7 × 7 storage grid, 37 cells, center (3, 3)).
fn hex_premiums(overrides: &[(u8, u8, Premium)]) -> Vec<Premium> {
    let mut premiums = Vec::new();
    for row in 0..7_u8 {
        for column in 0..7_u8 {
            if (3..=9).contains(&(row + column)) {
                premiums.push(
                    overrides
                        .iter()
                        .find(|(r, c, _)| (*r, *c) == (row, column))
                        .map_or(Premium::Normal, |(_, _, premium)| *premium),
                );
            }
        }
    }
    premiums
}

#[test]
fn hex_main_word_length_counts_cells_or_spelled_letters() {
    // Board: A A at (2, 4) (2, 5) and SCH at (3, 3). One new A at (2, 3), on a double word,
    // forms Horizontal AAA (3 cells, 3 letters) and Vertical A+SCH = ASCH (2 cells, 4 letters).
    // Under main-word-only premiums:
    //   tiles   → main AAA  (1 + 1 + 1)·2 = 6, cross ASCH 1 + 8 = 9  → 15
    //   letters → main ASCH (1 + 8)·2 = 18,    cross AAA 3           → 21
    for (metric, main, words, directions_expected, scores, total) in [
        (
            WordLengthMetric::Tiles,
            "AAA",
            ["AAA", "ASCH"],
            [Direction::Horizontal, Direction::Vertical],
            [6, 9],
            15,
        ),
        (
            WordLengthMetric::Letters,
            "ASCH",
            ["ASCH", "AAA"],
            [Direction::Vertical, Direction::Horizontal],
            [18, 3],
            21,
        ),
    ] {
        let mut rules = english_fixture();
        rules.identity.name = "multi-letter-hex-acceptance".into();
        rules.dictionary = words_identity();
        rules.board_shape = BoardShape::Hexagon;
        rules.board_size = 7;
        rules.premiums = hex_premiums(&[(2, 3, Premium::DoubleWord)]);
        rules.premium_policy = PremiumPolicy::MainWordOnly;
        rules.main_word_length = metric;
        rules.tiles = vec![definition("A", 60, 1), definition("SCH", 20, SCH_VALUE)];
        rules.validate().unwrap();
        let mut state = game(&rules, &[("A", 1)], &[("SCH", 1), ("A", 2)]);
        let seeded = tiles(&state, P2, &[(3, 3, "SCH"), (2, 4, "A"), (2, 5, "A")]);
        state.place_tiles(P2, &seeded).unwrap();

        let placement = tiles(&state, P1, &[(2, 3, "A")]);
        let validated = validate(&state, P1, &placement).unwrap();
        assert_eq!(
            validated
                .words()
                .iter()
                .map(|word| word.tiles().len())
                .collect::<Vec<_>>(),
            if main == "AAA" { [3, 2] } else { [2, 3] },
            "{metric:?}"
        );
        let preview = calculate(&validated).unwrap();
        assert_eq!(preview.main_word().text(), main, "{metric:?}");
        assert_eq!(texts(&preview), words, "{metric:?}");
        assert_eq!(directions(&preview), directions_expected, "{metric:?}");
        assert_eq!(preview.word_scores(), scores, "{metric:?}");
        assert_eq!(preview.total(), total, "{metric:?}");
        let committed = commit::apply_with_dictionary(
            &mut state,
            P1,
            &placement,
            &dictionary(&["AAA", "ASCH"]),
        )
        .unwrap();
        assert_eq!(committed, preview);
        assert_eq!(
            last_action(&state),
            TurnAction::Commit {
                words: words.into_iter().map(String::from).collect(),
                move_score: total,
                blank_count: 0,
            }
        );
    }
}

fn session(rules: &Ruleset, seed: u8) -> GameSession {
    let state = GameState::new(GameId::new(1), rules.clone(), [seed; 32]).unwrap();
    let mut session = GameSession::with_dictionary(state, Arc::new(dictionary(WORDS)));
    session.add_player(P1, "Ada".into()).unwrap();
    session.add_player(P2, "Lin".into()).unwrap();
    session
        .apply(P1, 1, &envelope(P1, 1, 0, Command::Start {}))
        .unwrap();
    session
}

#[test]
fn multi_letter_matches_serialize_reconstruct_and_replay_canonically() {
    let rules = square_rules(7);
    let text = serde_json::to_string(&rules).unwrap();
    assert!(text.contains(r#"{"kind":"letters","letters":"CH"}"#));
    let reconstructed: Ruleset = serde_json::from_str(&text).unwrap();
    assert_eq!(reconstructed, rules);

    let seed = seed_for(
        &rules,
        &[("CH", 1), ("A", 1), ("T", 1)],
        &[("CH", 1), ("A", 1)],
    );
    let mut original = session(&reconstructed, seed);
    let mut log = vec![(P1, 1, envelope(P1, 1, 0, Command::Start {}))];
    let mut run = |session: &mut GameSession, entry: (PlayerId, u32, CommandEnvelope)| {
        session.apply(entry.0, entry.1, &entry.2).unwrap();
        log.push(entry);
    };

    let opening = tiles(original.state(), P1, &OPENING);
    run(
        &mut original,
        (
            P1,
            2,
            envelope(
                P1,
                2,
                0,
                Command::Commit {
                    placements: opening,
                },
            ),
        ),
    );
    // The private rack carries the multi-letter face; the preview carries its spelled face.
    let rack = serde_json::to_value(original.player_snapshot(P2).unwrap()).unwrap();
    assert!(
        rack["own_rack"]["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tile| tile["face"] == json!({"kind": "letters", "letters": "CH"}))
    );
    let reply = tiles(original.state(), P2, &REPLY);
    run(
        &mut original,
        (
            P2,
            1,
            envelope(
                P2,
                1,
                1,
                Command::Preview {
                    placements: reply.clone(),
                },
            ),
        ),
    );
    assert!(
        serde_json::to_value(original.public_snapshot())
            .unwrap()
            .get("preview")
            .is_none()
    );
    let preview = serde_json::to_value(original.player_snapshot(P2).unwrap()).unwrap();
    let mut previewed: Vec<_> = preview["own_preview"]["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tile| {
            (
                tile["coordinate"]["row"].as_u64().unwrap(),
                tile["coordinate"]["column"].as_u64().unwrap(),
                tile["letter"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    previewed.sort();
    assert_eq!(previewed, [(8, 8, "CH".to_owned()), (8, 9, "A".to_owned())]);
    run(
        &mut original,
        (
            P2,
            2,
            envelope(P2, 2, 1, Command::Commit { placements: reply }),
        ),
    );
    run(&mut original, (P1, 3, envelope(P1, 3, 2, Command::Pass {})));

    let snapshot = original.public_snapshot();
    assert_eq!(snapshot.configuration, rules);
    assert_eq!(
        snapshot
            .players
            .iter()
            .map(|player| player.score)
            .collect::<Vec<_>>(),
        [14, 34]
    );
    assert_eq!(
        wire_board(&serde_json::to_value(&snapshot).unwrap()),
        [
            (7, 7, "CH".to_owned(), false),
            (7, 8, "A".to_owned(), false),
            (7, 9, "T".to_owned(), false),
            (8, 8, "CH".to_owned(), false),
            (8, 9, "A".to_owned(), false),
        ]
    );
    assert_eq!(
        snapshot
            .history
            .iter()
            .map(|turn| turn.action.clone())
            .collect::<Vec<_>>(),
        [
            TurnAction::Commit {
                words: vec!["CHAT".into()],
                move_score: 14,
                blank_count: 0,
            },
            TurnAction::Commit {
                words: vec!["CHA".into(), "ACH".into(), "TA".into()],
                move_score: 34,
                blank_count: 0,
            },
            TurnAction::Pass {},
        ]
    );

    let canonical = original.state().canonical_bytes().unwrap();
    let evidence: Value = serde_json::from_slice(&canonical).unwrap();
    assert_eq!(evidence["version"], 3);
    let from_evidence: Ruleset = serde_json::from_value(evidence["ruleset"].clone()).unwrap();
    assert_eq!(from_evidence, rules);

    // Replay the accepted commands on a match reconstructed from the canonical ruleset.
    let mut replay = GameSession::with_dictionary(
        GameState::new(GameId::new(1), from_evidence, [seed; 32]).unwrap(),
        Arc::new(dictionary(WORDS)),
    );
    replay.add_player(P1, "Ada".into()).unwrap();
    replay.add_player(P2, "Lin".into()).unwrap();
    for (player, sequence, command) in &log {
        replay.apply(*player, *sequence, command).unwrap();
    }
    assert_eq!(replay.state().canonical_bytes().unwrap(), canonical);
    assert_eq!(replay.public_snapshot(), snapshot);
    assert_eq!(
        replay.player_snapshot(P2).unwrap(),
        original.player_snapshot(P2).unwrap()
    );
    replay.state().verify_tile_conservation().unwrap();
}

/// Accepts every word, so the pinned fixture never depends on the authored word list.
struct AcceptAll(ContentIdentity);
impl Dictionary for AcceptAll {
    fn identity(&self) -> &ContentIdentity {
        &self.0
    }
    fn contains(&self, _: &str) -> bool {
        true
    }
}
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
fn pinned(bytes: &[u8]) -> (usize, u64) {
    (bytes.len(), fnv1a(bytes))
}

/// Lengths and FNV-1a hashes were captured on the pre-#116 baseline 2473b12.
/// Single-letter rulesets, a committed blank, an exchange and a live preview stay
/// byte-identical in the ruleset JSON, canonical evidence and public and player snapshots.
#[test]
fn single_letter_rulesets_states_and_snapshots_stay_byte_identical() {
    for rules in [english_fixture(), german_fixture()] {
        let text = serde_json::to_string(&rules).unwrap();
        assert!(
            !text.contains("letters"),
            "single-letter JSON gained letters"
        );
    }
    assert_eq!(
        pinned(&serde_json::to_vec(&english_fixture()).unwrap()),
        (4293, 0x79e3_f8d6_9ef3_c050)
    );
    assert_eq!(
        pinned(&serde_json::to_vec(&german_fixture()).unwrap()),
        (1403, 0xab55_aea4_ccde_0bd4)
    );

    let start = |seed: u8| {
        let rules = english_fixture();
        let dictionary = Arc::new(AcceptAll(rules.dictionary.clone()));
        let mut session = GameSession::with_dictionary(
            GameState::new(GameId::new(1), rules, [seed; 32]).unwrap(),
            dictionary,
        );
        session.add_player(P1, "Ada".into()).unwrap();
        session.add_player(P2, "Lin".into()).unwrap();
        session
            .apply(P1, 1, &envelope(P1, 1, 0, Command::Start {}))
            .unwrap();
        session
    };
    let seed = (0..=255)
        .find(|&seed| {
            start(seed).state().rack(P1).unwrap()[..2]
                .iter()
                .any(|tile| tile.face() == TileFace::Blank {})
        })
        .unwrap();
    assert_eq!(seed, 3);
    let mut session = start(seed);
    let place = |session: &GameSession, player: PlayerId, cells: &[(usize, u8, u8, char)]| {
        let rack = session.state().rack(player).unwrap();
        cells
            .iter()
            .map(|&(index, row, column, blank)| Placement {
                tile_id: rack[index].id(),
                coordinate: coordinate(row, column),
                blank_as: (rack[index].face() == TileFace::Blank {}).then_some(blank),
            })
            .collect::<Vec<_>>()
    };
    let opening = place(&session, P1, &[(0, 7, 7, 'A'), (1, 7, 8, 'A')]);
    assert_eq!(
        opening
            .iter()
            .filter(|placement| placement.blank_as.is_some())
            .count(),
        1
    );
    session
        .apply(
            P1,
            2,
            &envelope(
                P1,
                2,
                0,
                Command::Commit {
                    placements: opening,
                },
            ),
        )
        .unwrap();
    let tile = session.state().rack(P2).unwrap()[0].id();
    session
        .apply(
            P2,
            1,
            &envelope(
                P2,
                1,
                1,
                Command::Exchange {
                    tile_ids: vec![tile],
                },
            ),
        )
        .unwrap();
    let preview = place(&session, P1, &[(0, 6, 7, 'B')]);
    session
        .apply(
            P1,
            3,
            &envelope(
                P1,
                3,
                2,
                Command::Preview {
                    placements: preview,
                },
            ),
        )
        .unwrap();
    assert!(session.player_snapshot(P1).unwrap().own_preview.is_some());
    assert_eq!(
        pinned(&session.state().canonical_bytes().unwrap()),
        (12052, 0x30db_b2d4_443a_9286)
    );
    assert_eq!(
        pinned(&serde_json::to_vec(&session.public_snapshot()).unwrap()),
        (5179, 0xda0e_9947_03ab_8360)
    );
    assert_eq!(
        pinned(&serde_json::to_vec(&session.player_snapshot(P1).unwrap()).unwrap()),
        (5697, 0xe985_f3ce_b69c_3b77)
    );
}

/// #96's browser trace is proved through accepted commands, not a pre-seeded board.
/// The fifth move also pins the Vertical-before-Diagonal equal-cell tie-break.
#[test]
fn hex_browser_trace_is_legal_with_combined_faces_under_both_policies_and_metrics() {
    let words = dictionary(&[
        "SCHA", "AA", "ASCH", "AAA", "AAAA", "ASCHCH", "AACH", "ACH", "ASCHA",
    ]);
    for policy in [PremiumPolicy::AllWord, PremiumPolicy::MainWordOnly] {
        for metric in [WordLengthMetric::Tiles, WordLengthMetric::Letters] {
            let mut rules = english_fixture();
            rules.identity.name = "hex-browser-trace".into();
            rules.dictionary = words_identity();
            rules.board_shape = BoardShape::Hexagon;
            rules.board_size = 7;
            rules.premiums = hex_premiums(&[(2, 3, Premium::DoubleWord)]);
            rules.premium_policy = policy;
            rules.main_word_length = metric;
            rules.tiles = vec![
                definition("A", 60, 1),
                definition("CH", 20, CH_VALUE),
                definition("SCH", 20, SCH_VALUE),
                definition("?", 10, 0),
                definition("T", 10, 1),
            ];
            let mut state = game(
                &rules,
                &[("A", 2), ("CH", 1), ("SCH", 1)],
                &[("A", 3), ("?", 1)],
            );
            let off_center = tiles(&state, P1, &[(0, 3, "SCH"), (0, 4, "A")]);
            let untouched = state.canonical_bytes().unwrap();
            assert!(matches!(
                validate(&state, P1, &off_center),
                Err(PlacementError::CenterRequired)
            ));
            assert_eq!(state.canonical_bytes().unwrap(), untouched);

            let opening = tiles(&state, P1, &[(3, 3, "SCH"), (3, 4, "A")]);
            let first = commit::apply_with_dictionary(&mut state, P1, &opening, &words).unwrap();
            assert_eq!(texts(&first), ["SCHA"]);
            assert_eq!(first.total(), 9);

            let mut invalid = tiles(&state, P2, &[(2, 4, "A"), (2, 5, "?")]);
            invalid[1].blank_as = Some('T');
            let untouched = state.canonical_bytes().unwrap();
            assert!(matches!(
                commit::apply_with_dictionary(&mut state, P2, &invalid, &words),
                Err(CommitError::Dictionary(_))
            ));
            assert_eq!(state.canonical_bytes().unwrap(), untouched);

            let crossing = tiles(&state, P2, &[(2, 4, "A"), (2, 5, "A")]);
            let second = commit::apply_with_dictionary(&mut state, P2, &crossing, &words).unwrap();
            assert_eq!(texts(&second), ["AA", "AA", "ASCH", "AA"]);
            assert_eq!(second.word_scores(), [2, 2, 9, 2]);
            assert_eq!(second.total(), 15);

            let intersection = tiles(&state, P1, &[(2, 3, "A")]);
            let preview = calculate(&validate(&state, P1, &intersection).unwrap()).unwrap();
            let (main, scores, total) = match (policy, metric) {
                (PremiumPolicy::AllWord, WordLengthMetric::Tiles) => ("AAA", [6, 18], 24),
                (PremiumPolicy::AllWord, WordLengthMetric::Letters) => ("ASCH", [18, 6], 24),
                (PremiumPolicy::MainWordOnly, WordLengthMetric::Tiles) => ("AAA", [6, 9], 15),
                (PremiumPolicy::MainWordOnly, WordLengthMetric::Letters) => ("ASCH", [18, 3], 21),
            };
            assert_eq!(preview.main_word().text(), main);
            assert_eq!(preview.word_scores(), scores);
            assert_eq!(preview.total(), total);
            assert_eq!(
                commit::apply_with_dictionary(&mut state, P1, &intersection, &words).unwrap(),
                preview
            );

            let extension = tiles(&state, P2, &[(2, 6, "A")]);
            let fourth = commit::apply_with_dictionary(&mut state, P2, &extension, &words).unwrap();
            assert_eq!(texts(&fourth), ["AAAA"]);
            assert_eq!(fourth.total(), 4); // spent double-word is not reused

            let ch = tiles(&state, P1, &[(4, 3, "CH")]);
            let fifth = commit::apply_with_dictionary(&mut state, P1, &ch, &words).unwrap();
            assert_eq!(texts(&fifth), ["ASCHCH", "AACH"]);
            assert_eq!(
                directions(&fifth),
                [Direction::Vertical, Direction::Diagonal]
            );
            assert_eq!(fifth.word_scores(), [14, 7]);
            assert_eq!(fifth.total(), 21);

            let mut blank = tiles(&state, P2, &[(4, 2, "?")]);
            blank[0].blank_as = Some('A');
            let sixth = commit::apply_with_dictionary(&mut state, P2, &blank, &words).unwrap();
            assert_eq!(texts(&sixth), ["ASCHA", "ACH"]);
            assert_eq!(sixth.word_scores(), [9, 5]);
            assert_eq!(sixth.total(), 14);
            state.verify_tile_conservation().unwrap();
        }
    }
}
