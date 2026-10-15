//! Independent acceptance contract for authoritative hexagonal board rules (#110).
//! Specification: docs/specs/hexagonal-board.md. Uses only the public engine API.
//!
//! Hex boards of radius R store axial cells in the existing `Coordinate` grid of side
//! 2R + 1: `(row, column)` is on the board iff `R <= row + column <= 3R`. The three axes
//! step by `(0, +1)` Horizontal, `(+1, 0)` Vertical and `(+1, -1)` Diagonal. These tests
//! deliberately never depend on the provisional reading direction of the Diagonal axis.
use scrabble_game::{
    commit::{self, CommitError},
    dictionary::{DictionaryError, WordList},
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    placement::{Direction, PlacementError, validate},
    protocol::{Command, CommandEnvelope, Phase, Placement, TurnAction},
    ruleset::{
        BoardShape, ContentIdentity, Premium, PremiumPolicy, Ruleset, TileDefinition, TileFace,
        WordLengthMetric, english_fixture, german_fixture,
    },
    scoring::{MoveScore, calculate, commit_placement},
    session::GameSession,
    state::{GameState, StateError},
};
use std::{collections::BTreeSet, sync::Arc};

/// Radius of the fixture board: a 7 × 7 storage grid holding 37 cells, centered at (3, 3).
const R: u8 = 3;
const A_VALUE: u32 = 1;
const B_VALUE: u32 = 3;
/// No `AB`/`BA`, so the only invalid formed word in the fixtures lies on the Diagonal axis.
const STRICT_WORDS: &[&str] = &["AA", "AAA", "BAA", "ABA"];
const ALL_WORDS: &[&str] = &["AA", "AAA", "BAA", "ABA", "AB", "BA"];

fn on_board(row: u8, column: u8) -> bool {
    row <= 2 * R && column <= 2 * R && (R..=3 * R).contains(&(row + column))
}
/// One premium per on-board cell, in row-major storage order with off-board cells skipped.
fn hex_premiums(radius: u8, overrides: &[(u8, u8, Premium)]) -> Vec<Premium> {
    let side = 2 * radius + 1;
    let mut premiums = Vec::new();
    for row in 0..side {
        for column in 0..side {
            if (radius..=3 * radius).contains(&(row + column)) {
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
fn words_identity() -> ContentIdentity {
    ContentIdentity {
        name: "hex-acceptance-words".into(),
        revision: "1".into(),
    }
}
fn dictionary(words: &[&str]) -> WordList {
    WordList::from_text(words_identity(), &words.join("\n")).unwrap()
}
fn hex_rules(policy: PremiumPolicy, premiums: &[(u8, u8, Premium)]) -> Ruleset {
    let mut rules = english_fixture();
    rules.identity.name = "hex-acceptance".into();
    rules.dictionary = words_identity();
    rules.board_shape = BoardShape::Hexagon;
    rules.premium_policy = policy;
    rules.main_word_length = WordLengthMetric::Tiles;
    rules.board_size = 2 * R + 1;
    rules.premiums = hex_premiums(R, premiums);
    rules.rack_size = 7;
    rules.tiles = vec![
        TileDefinition {
            face: TileFace::Letter { letter: 'A' },
            count: 60,
            value: A_VALUE as u16,
        },
        TileDefinition {
            face: TileFace::Letter { letter: 'B' },
            count: 40,
            value: B_VALUE as u16,
        },
    ];
    rules
}
fn count(state: &GameState, player: u64, letter: char) -> usize {
    state
        .rack(PlayerId::new(player))
        .unwrap()
        .iter()
        .filter(|tile| tile.face() == TileFace::Letter { letter })
        .count()
}
/// Deterministically selects the first seed whose dealt racks hold the requested (A, B) counts.
fn game(rules: Ruleset, first: (usize, usize), second: (usize, usize)) -> GameState {
    (0..=255)
        .find_map(|seed| {
            let mut state = GameState::new(GameId::new(1), rules.clone(), [seed; 32]).unwrap();
            state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
            state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
            state.deal_initial_racks().unwrap();
            let enough = |player, (a, b): (usize, usize)| {
                count(&state, player, 'A') >= a && count(&state, player, 'B') >= b
            };
            (enough(1, first) && enough(2, second)).then_some(state)
        })
        .expect("deterministic hex rack fixture")
}
fn tiles(state: &GameState, player: u64, cells: &[(u8, u8, char)]) -> Vec<Placement> {
    let rack = state.rack(PlayerId::new(player)).unwrap();
    let mut used = BTreeSet::new();
    cells
        .iter()
        .map(|&(row, column, letter)| {
            let tile = rack
                .iter()
                .find(|tile| {
                    tile.face() == TileFace::Letter { letter } && !used.contains(&tile.id())
                })
                .expect("fixture rack holds the requested letter");
            used.insert(tile.id());
            Placement {
                tile_id: tile.id(),
                coordinate: Coordinate::new(row, column).unwrap(),
                blank_as: None,
            }
        })
        .collect()
}
/// Storage-level board setup by the non-active player; establishes no move legality.
fn seed_board(state: &mut GameState, cells: &[(u8, u8, char)]) {
    let placements = tiles(state, 2, cells);
    state.place_tiles(PlayerId::new(2), &placements).unwrap();
}
fn rejected(state: &GameState, placements: &[Placement], error: PlacementError) {
    let before = state.canonical_bytes().unwrap();
    assert_eq!(
        validate(state, PlayerId::new(1), placements).err(),
        Some(error)
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
}
fn score(state: &GameState, player: u64, placements: &[Placement]) -> MoveScore {
    calculate(&validate(state, PlayerId::new(player), placements).unwrap()).unwrap()
}
fn directions(score: &MoveScore) -> Vec<Direction> {
    score.words().iter().map(|word| word.direction()).collect()
}
fn last_commit_words(state: &GameState) -> Vec<String> {
    match &state.public_snapshot().history.last().unwrap().action {
        TurnAction::Commit { words, .. } => words.clone(),
        other => panic!("expected a commit, got {other:?}"),
    }
}

#[test]
fn hex_geometry_is_validated_with_one_premium_per_on_board_cell() {
    let rules = hex_rules(PremiumPolicy::AllWord, &[]);
    assert_eq!(rules.premiums.len(), 37); // 3R² + 3R + 1
    assert_eq!(
        (0..7)
            .flat_map(|row| (0..7).map(move |column| (row, column)))
            .filter(|&(row, column)| on_board(row, column))
            .count(),
        37
    );
    rules.validate().unwrap();
    GameState::new(GameId::new(1), rules.clone(), [1; 32]).unwrap();

    // Smallest and largest supported radius (the existing 15-cell coordinate bound).
    for (size, cells) in [(3, 7), (15, 169)] {
        let mut valid = rules.clone();
        valid.board_size = size;
        valid.premiums = hex_premiums(size / 2, &[]);
        assert_eq!(valid.premiums.len(), cells);
        valid.validate().unwrap();
    }
    // Wrong premium counts, including a full square grid of the same side.
    for length in [36, 38, 49] {
        let mut invalid = rules.clone();
        invalid.premiums = vec![Premium::Normal; length];
        assert!(invalid.validate().is_err(), "premium count {length}");
        assert!(matches!(
            GameState::new(GameId::new(1), invalid, [1; 32]),
            Err(StateError::InvalidRuleset(_))
        ));
    }
    // Radius 0, even storage side and a radius beyond the coordinate bound.
    for (size, length) in [(1, 1), (8, 37), (8, 64), (17, 217)] {
        let mut invalid = rules.clone();
        invalid.board_size = size;
        invalid.premiums = vec![Premium::Normal; length];
        assert!(invalid.validate().is_err(), "board size {size}");
    }
    // Hex-only board-profile settings are never silently ignored on a square board.
    english_fixture().validate().unwrap();
    let mut square = english_fixture();
    square.premium_policy = PremiumPolicy::MainWordOnly;
    assert!(square.validate().is_err());
    let mut square = english_fixture();
    square.main_word_length = WordLengthMetric::Letters;
    assert!(square.validate().is_err());
    let mut hex = hex_rules(PremiumPolicy::MainWordOnly, &[]);
    hex.main_word_length = WordLengthMetric::Letters;
    hex.validate().unwrap();
}

#[test]
fn off_board_hex_cells_are_rejected_without_mutation() {
    let mut state = game(hex_rules(PremiumPolicy::AllWord, &[]), (3, 0), (0, 0));
    let rack = state.rack(PlayerId::new(1)).unwrap().to_vec();
    for (row, column) in [(0, 0), (0, 2), (2, 0), (6, 6), (4, 6), (7, 3), (3, 7)] {
        assert!(!on_board(row, column));
        let placement = Placement {
            tile_id: rack[0].id(),
            coordinate: Coordinate::new(row, column).unwrap(),
            blank_as: None,
        };
        rejected(
            &state,
            std::slice::from_ref(&placement),
            PlacementError::Storage(StateError::OutsideBoard),
        );
        let before = state.canonical_bytes().unwrap();
        assert_eq!(
            state.place_tiles(PlayerId::new(1), &[placement]),
            Err(StateError::OutsideBoard)
        );
        assert_eq!(state.canonical_bytes().unwrap(), before);
    }
    // A line partly outside the hexagon is rejected as a whole.
    let line = tiles(&state, 1, &[(0, 1, 'A'), (0, 2, 'A'), (0, 3, 'A')]);
    rejected(
        &state,
        &line,
        PlacementError::Storage(StateError::OutsideBoard),
    );
}

#[test]
fn opening_word_must_cover_the_center_hexagon() {
    let mut state = game(hex_rules(PremiumPolicy::AllWord, &[]), (3, 0), (0, 0));
    // (0, 3) and (0, 4) are corner-row cells on the board, but not the center.
    assert!(on_board(0, 3) && on_board(0, 4));
    rejected(
        &state,
        &tiles(&state, 1, &[(0, 3, 'A'), (0, 4, 'A')]),
        PlacementError::CenterRequired,
    );
    rejected(
        &state,
        &tiles(&state, 1, &[(3, 3, 'A')]),
        PlacementError::NoWord,
    );
    let opening = tiles(&state, 1, &[(3, 4, 'A'), (3, 3, 'A')]);
    let validated = validate(&state, PlayerId::new(1), &opening).unwrap();
    assert_eq!(validated.words().len(), 1);
    assert_eq!(validated.words()[0].text(), "AA");
    assert_eq!(validated.words()[0].direction(), Direction::Horizontal);
    let result = commit::apply_with_dictionary(
        &mut state,
        PlayerId::new(1),
        &opening,
        &dictionary(STRICT_WORDS),
    )
    .unwrap();
    assert_eq!(result.total(), 2 * A_VALUE);
    assert!(state.board_tile(Coordinate::new(3, 3).unwrap()).is_some());
    assert!(state.board_tile(Coordinate::new(3, 4).unwrap()).is_some());
    state.verify_tile_conservation().unwrap();
}

#[test]
fn words_extend_along_each_of_the_three_hex_axes() {
    type AxisCase<'a> = (Direction, [(u8, u8, char); 3], Option<&'a str>);
    let cases: [AxisCase<'_>; 3] = [
        (
            Direction::Horizontal,
            [(3, 2, 'B'), (3, 3, 'A'), (3, 4, 'A')],
            Some("BAA"),
        ),
        (
            Direction::Vertical,
            [(2, 3, 'B'), (3, 3, 'A'), (4, 3, 'A')],
            Some("BAA"),
        ),
        // Palindrome: independent of the provisional Diagonal reading direction.
        (
            Direction::Diagonal,
            [(2, 4, 'A'), (3, 3, 'B'), (4, 2, 'A')],
            Some("ABA"),
        ),
    ];
    for (direction, cells, text) in cases {
        let mut state = game(hex_rules(PremiumPolicy::AllWord, &[]), (2, 1), (0, 0));
        // Input order does not matter.
        let mut placements = tiles(&state, 1, &cells);
        placements.reverse();
        let validated = validate(&state, PlayerId::new(1), &placements).unwrap();
        assert_eq!(validated.words().len(), 1, "{direction:?}");
        let word = &validated.words()[0];
        assert_eq!(word.direction(), direction);
        assert_eq!(Some(word.text().as_str()), text);
        let covered: BTreeSet<_> = word
            .tiles()
            .iter()
            .map(|tile| (tile.coordinate().row(), tile.coordinate().column()))
            .collect();
        let expected: BTreeSet<_> = cells
            .iter()
            .map(|&(row, column, _)| (row, column))
            .collect();
        assert_eq!(covered, expected);
        if direction != Direction::Diagonal {
            // Horizontal/Vertical keep the square reading order: increasing column, then row.
            let order: Vec<_> = word
                .tiles()
                .iter()
                .map(|tile| (tile.coordinate().row(), tile.coordinate().column()))
                .collect();
            let cells_in_order: Vec<_> = cells
                .iter()
                .map(|&(row, column, _)| (row, column))
                .collect();
            assert_eq!(order, cells_in_order);
        }
        let result = commit::apply_with_dictionary(
            &mut state,
            PlayerId::new(1),
            &placements,
            &dictionary(STRICT_WORDS),
        )
        .unwrap();
        assert_eq!(result.total(), B_VALUE + 2 * A_VALUE);
        assert_eq!(result.main_word().direction(), direction);
        assert_eq!(last_commit_words(&state), vec![text.unwrap().to_owned()]);
        state.verify_tile_conservation().unwrap();
    }
    // Horizontal and Vertical words are read in square order, so the reverse spelling is a
    // different (here invalid) word.
    for cells in [
        [(3, 2, 'A'), (3, 3, 'A'), (3, 4, 'B')],
        [(2, 3, 'A'), (3, 3, 'A'), (4, 3, 'B')],
    ] {
        let mut state = game(hex_rules(PremiumPolicy::AllWord, &[]), (2, 1), (0, 0));
        let placements = tiles(&state, 1, &cells);
        let before = state.canonical_bytes().unwrap();
        assert_eq!(
            commit::apply_with_dictionary(
                &mut state,
                PlayerId::new(1),
                &placements,
                &dictionary(STRICT_WORDS)
            ),
            Err(CommitError::Dictionary(DictionaryError::InvalidWords {
                words: vec!["AAB".into()]
            }))
        );
        assert_eq!(state.canonical_bytes().unwrap(), before);
    }
}

/// Atlas 1.43: down-left reads from top-right to bottom-left, never whichever spelling
/// happens to be in the dictionary. Non-palindromic input pins the owner decision.
#[test]
fn diagonal_reads_top_to_bottom_and_never_substitutes_the_reverse_word() {
    let cells = [(2, 4, 'B'), (3, 3, 'A'), (4, 2, 'A')];
    for policy in [PremiumPolicy::AllWord, PremiumPolicy::MainWordOnly] {
        for reverse_input in [false, true] {
            let mut state = game(hex_rules(policy, &[]), (2, 1), (0, 0));
            let mut placements = tiles(&state, 1, &cells);
            if reverse_input {
                placements.reverse();
            }
            let before = state.canonical_bytes().unwrap();
            // A valid reverse spelling cannot rescue the invalid actual top-to-bottom word.
            assert_eq!(
                commit::apply_with_dictionary(
                    &mut state,
                    PlayerId::new(1),
                    &placements,
                    &dictionary(&["AAB"]),
                ),
                Err(CommitError::Dictionary(DictionaryError::InvalidWords {
                    words: vec!["BAA".into()],
                })),
            );
            assert_eq!(state.canonical_bytes().unwrap(), before);
            let result = commit::apply_with_dictionary(
                &mut state,
                PlayerId::new(1),
                &placements,
                &dictionary(&["BAA"]),
            )
            .unwrap();
            assert_eq!(result.main_word().text(), "BAA");
            assert_eq!(result.main_word().direction(), Direction::Diagonal);
            assert_eq!(result.total(), B_VALUE + 2 * A_VALUE);
            assert_eq!(last_commit_words(&state), ["BAA"]);
            state.verify_tile_conservation().unwrap();
        }
    }
}

#[test]
fn bent_square_diagonal_gapped_and_disconnected_placements_are_rejected() {
    let mut state = game(hex_rules(PremiumPolicy::AllWord, &[]), (3, 0), (2, 0));
    // Bent across two axes, and a square diagonal that is not a hex axis.
    rejected(
        &state,
        &tiles(&state, 1, &[(3, 2, 'A'), (3, 3, 'A'), (4, 3, 'A')]),
        PlacementError::NotLine,
    );
    rejected(
        &state,
        &tiles(&state, 1, &[(3, 3, 'A'), (4, 4, 'A')]),
        PlacementError::NotLine,
    );
    seed_board(&mut state, &[(3, 3, 'A'), (3, 4, 'A')]);
    // Diagonal line (row + column = 6) with the empty cell (2, 4) between the new tiles.
    rejected(
        &state,
        &tiles(&state, 1, &[(1, 5, 'A'), (4, 2, 'A')]),
        PlacementError::Gap,
    );
    // Same axis, but the committed center tile fills the only gap.
    let filled = tiles(&state, 1, &[(4, 2, 'A'), (2, 4, 'A')]);
    let validated = validate(&state, PlayerId::new(1), &filled).unwrap();
    let words = validated.words();
    assert_eq!(words.len(), 2);
    assert_eq!(words[0].direction(), Direction::Diagonal);
    assert_eq!(words[0].tiles().len(), 3);
    assert_eq!(
        words[0].tiles().iter().filter(|tile| tile.is_new()).count(),
        2
    );
    assert_eq!(words[1].direction(), Direction::Vertical); // (2, 4) over committed (3, 4)
    assert_eq!(words[1].text(), "AA");
    // (2, 2) touches (3, 3) only square-diagonally: no hex adjacency to the board.
    rejected(
        &state,
        &tiles(&state, 1, &[(2, 1, 'A'), (2, 2, 'A')]),
        PlacementError::Disconnected,
    );
    rejected(
        &state,
        &tiles(&state, 1, &[(5, 0, 'A'), (5, 1, 'A')]),
        PlacementError::Disconnected,
    );
}

/// Committed B at (3, 3) and A at (3, 4); player 1 then plays A at (2, 4) and (2, 5).
/// Formed words: main Horizontal (2,4)-(2,5); Vertical (2,4)-(3,4);
/// Diagonal (2,4)-(3,3) (the only word containing B); Diagonal (2,5)-(3,4).
fn crossing_fixture(
    policy: PremiumPolicy,
    premiums: &[(u8, u8, Premium)],
) -> (GameState, Vec<Placement>) {
    let mut state = game(hex_rules(policy, premiums), (2, 0), (2, 1));
    seed_board(&mut state, &[(3, 3, 'B'), (3, 4, 'A')]);
    let placements = tiles(&state, 1, &[(2, 5, 'A'), (2, 4, 'A')]);
    (state, placements)
}

#[test]
fn an_invalid_cross_word_on_the_third_axis_rejects_the_whole_move_without_mutation() {
    let (mut state, placements) = crossing_fixture(PremiumPolicy::AllWord, &[]);
    let before = state.canonical_bytes().unwrap();
    let snapshot = state.player_snapshot(PlayerId::new(1)).unwrap();
    let result = commit::apply_with_dictionary(
        &mut state,
        PlayerId::new(1),
        &placements,
        &dictionary(STRICT_WORDS),
    );
    let words = match result {
        Err(CommitError::Dictionary(DictionaryError::InvalidWords { words })) => words,
        other => panic!("expected a dictionary rejection, got {other:?}"),
    };
    assert!(
        words == ["AB"] || words == ["BA"],
        "only the Diagonal word through B is invalid: {words:?}"
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
    assert_eq!(state.player_snapshot(PlayerId::new(1)).unwrap(), snapshot);

    let result = commit::apply_with_dictionary(
        &mut state,
        PlayerId::new(1),
        &placements,
        &dictionary(ALL_WORDS),
    )
    .unwrap();
    assert_eq!(
        directions(&result),
        [
            Direction::Horizontal,
            Direction::Vertical,
            Direction::Diagonal,
            Direction::Diagonal
        ]
    );
    let history = last_commit_words(&state);
    assert_eq!(history.len(), 4);
    assert_eq!(history[0], "AA");
    assert_eq!(history[1], "AA");
    assert!(history[2] == "AB" || history[2] == "BA");
    assert_eq!(history[3], "AA");
    assert_eq!(result.total(), 2 + 2 + 4 + 2);
    state.verify_tile_conservation().unwrap();
}

/// Double word under (2, 4), the intersection of three new words; triple letter under (2, 5),
/// the intersection of two. Values: A = 1, B = 3.
///
/// | word | all_word | main_word_only |
/// | --- | --- | --- |
/// | main H (2,4)(2,5) | (1 + 3·1) · 2 = 8 | (1 + 3·1) · 2 = 8 |
/// | V (2,4)(3,4) | (1 + 1) · 2 = 4 | 1 + 1 = 2 |
/// | D (2,4)(3,3) | (1 + 3) · 2 = 8 | 1 + 3 = 4 |
/// | D (2,5)(3,4) | 3·1 + 1 = 4 | 1 + 1 = 2 |
/// | total | 24 | 16 |
#[test]
fn every_new_word_scores_under_both_premium_policies_and_premiums_are_consumed() {
    let premiums = [(2, 4, Premium::DoubleWord), (2, 5, Premium::TripleLetter)];
    for (policy, expected, total) in [
        (PremiumPolicy::AllWord, [8, 4, 8, 4], 24),
        (PremiumPolicy::MainWordOnly, [8, 2, 4, 2], 16),
    ] {
        let (mut state, placements) = crossing_fixture(policy, &premiums);
        let preview = score(&state, 1, &placements);
        assert_eq!(preview.word_scores(), expected, "{policy:?}");
        assert_eq!(preview.total(), total);
        assert_eq!(preview.bingo_bonus(), 0);
        assert_eq!(preview.words().len(), 4);
        assert_eq!(
            preview
                .words()
                .iter()
                .map(|word| word.score())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(preview.main_word(), &preview.words()[0]);
        assert_eq!(preview.main_word().text(), "AA");
        assert_eq!(preview.main_word().direction(), Direction::Horizontal);
        assert_eq!(preview.main_word().score(), 8);

        let committed = commit::apply_with_dictionary(
            &mut state,
            PlayerId::new(1),
            &placements,
            &dictionary(ALL_WORDS),
        )
        .unwrap();
        assert_eq!(committed, preview);
        assert_eq!(state.public_snapshot().players[0].score, total as i32);

        // Next turn: player 2 extends the main word to (2, 6) (a Normal cell). The spent
        // premiums under (2, 4) and (2, 5) no longer apply: 1 + 1 + 1 = 3 under either policy.
        let extension = tiles(&state, 2, &[(2, 6, 'A')]);
        let later = commit::apply_with_dictionary(
            &mut state,
            PlayerId::new(2),
            &extension,
            &dictionary(ALL_WORDS),
        )
        .unwrap();
        assert_eq!(later.word_scores(), [3]);
        assert_eq!(later.main_word().text(), "AAA");
        assert_eq!(later.total(), 3);
        let players = state.public_snapshot().players;
        assert_eq!((players[0].score, players[1].score), (total as i32, 3));
        state.verify_tile_conservation().unwrap();
    }
}

/// A single A at (2, 4) on a double-word cell, with committed tiles around it.
fn single_tile(
    policy: PremiumPolicy,
    metric: WordLengthMetric,
    seeded: &[(u8, u8, char)],
) -> (GameState, Vec<Placement>) {
    let mut rules = hex_rules(policy, &[(2, 4, Premium::DoubleWord)]);
    rules.main_word_length = metric;
    let mut state = game(rules, (1, 0), (3, 1));
    seed_board(&mut state, seeded);
    let placement = tiles(&state, 1, &[(2, 4, 'A')]);
    (state, placement)
}

#[test]
fn a_single_tile_selects_the_longest_formed_word_as_main_word() {
    // Vertical (2,4)(3,4)(4,4) has three tiles; Diagonal (2,4)(3,3) has two.
    let seeded = [(3, 3, 'B'), (3, 4, 'A'), (4, 4, 'A')];
    for (policy, metric, expected, total) in [
        // main V: 3 · 2 = 6; D: 1 + 3 = 4. Choosing D instead would give 8 + 3 = 11.
        (
            PremiumPolicy::MainWordOnly,
            WordLengthMetric::Tiles,
            [6, 4],
            10,
        ),
        // Single-letter faces: the letter metric selects the same word.
        (
            PremiumPolicy::MainWordOnly,
            WordLengthMetric::Letters,
            [6, 4],
            10,
        ),
        // All-word: V 6; D (1 + 3) · 2 = 8.
        (PremiumPolicy::AllWord, WordLengthMetric::Tiles, [6, 8], 14),
    ] {
        let (mut state, placement) = single_tile(policy, metric, &seeded);
        let preview = score(&state, 1, &placement);
        assert_eq!(
            preview.main_word().direction(),
            Direction::Vertical,
            "{policy:?} {metric:?}"
        );
        assert_eq!(preview.main_word().text(), "AAA");
        assert_eq!(
            directions(&preview),
            [Direction::Vertical, Direction::Diagonal]
        );
        assert_eq!(preview.word_scores(), expected);
        assert_eq!(preview.total(), total);
        let committed = commit::apply_with_dictionary(
            &mut state,
            PlayerId::new(1),
            &placement,
            &dictionary(ALL_WORDS),
        )
        .unwrap();
        assert_eq!(committed, preview);
        assert_eq!(last_commit_words(&state)[0], "AAA");
    }
}

#[test]
fn equally_long_single_tile_words_use_the_fixed_axis_tie_break() {
    // Vertical "AA" and Diagonal (A, B) both have two tiles: Vertical precedes Diagonal.
    // Main-word-only: V (1 + 1) · 2 = 4, D 1 + 3 = 4 → 8. Choosing D would give 8 + 2 = 10.
    for (policy, expected, total) in [
        (PremiumPolicy::MainWordOnly, [4, 4], 8),
        (PremiumPolicy::AllWord, [4, 8], 12),
    ] {
        let (state, placement) =
            single_tile(policy, WordLengthMetric::Tiles, &[(3, 3, 'B'), (3, 4, 'A')]);
        let preview = score(&state, 1, &placement);
        assert_eq!(preview.main_word().direction(), Direction::Vertical);
        assert_eq!(
            directions(&preview),
            [Direction::Vertical, Direction::Diagonal]
        );
        assert_eq!(preview.word_scores(), expected);
        assert_eq!(preview.total(), total);
    }
    // Three equally long words: Horizontal precedes Vertical and Diagonal.
    // Main-word-only: H (1 + 1) · 2 = 4, V 2, D 4 → 10. All-word: 4 + 4 + 8 = 16.
    for (policy, expected, total) in [
        (PremiumPolicy::MainWordOnly, [4, 2, 4], 10),
        (PremiumPolicy::AllWord, [4, 4, 8], 16),
    ] {
        let (state, placement) = single_tile(
            policy,
            WordLengthMetric::Tiles,
            &[(3, 3, 'B'), (3, 4, 'A'), (2, 5, 'A')],
        );
        let preview = score(&state, 1, &placement);
        assert_eq!(preview.main_word().direction(), Direction::Horizontal);
        assert_eq!(preview.main_word().text(), "AA");
        assert_eq!(
            directions(&preview),
            [
                Direction::Horizontal,
                Direction::Vertical,
                Direction::Diagonal
            ]
        );
        assert_eq!(preview.word_scores(), expected);
        assert_eq!(preview.total(), total);
    }
}

fn envelope(player: u64, sequence: u32, turn: u64, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(player),
        sequence,
        expected_turn: TurnId::new(turn),
        command,
    }
}
fn new_session(rules: &Ruleset, seed: u8) -> GameSession {
    let state = GameState::new(GameId::new(1), rules.clone(), [seed; 32]).unwrap();
    let mut session = GameSession::with_dictionary(state, Arc::new(dictionary(ALL_WORDS)));
    session.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    session.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    session
        .apply(PlayerId::new(1), 1, &envelope(1, 1, 0, Command::Start {}))
        .unwrap();
    session
}
/// Opening along the Diagonal axis through the center onto a double word, a Vertical reply
/// and a pass. Returns the accepted command log.
fn play(session: &mut GameSession) -> Vec<(u64, u32, CommandEnvelope)> {
    let mut log = Vec::new();
    let opening = tiles(session.state(), 1, &[(2, 4, 'A'), (3, 3, 'A')]);
    log.push((
        1,
        2,
        envelope(
            1,
            2,
            0,
            Command::Commit {
                placements: opening,
            },
        ),
    ));
    for (player, sequence, command) in &log {
        session
            .apply(PlayerId::new(*player), *sequence, command)
            .unwrap();
    }
    let reply = tiles(session.state(), 2, &[(4, 3, 'A')]);
    let tail = [
        (
            2,
            1,
            envelope(2, 1, 1, Command::Commit { placements: reply }),
        ),
        (1, 3, envelope(1, 3, 2, Command::Pass {})),
    ];
    for (player, sequence, command) in &tail {
        session
            .apply(PlayerId::new(*player), *sequence, command)
            .unwrap();
    }
    log.extend(tail);
    log
}

#[test]
fn hex_match_serializes_reconstructs_and_replays_canonically() {
    let rules = hex_rules(PremiumPolicy::MainWordOnly, &[(2, 4, Premium::DoubleWord)]);
    let json = serde_json::to_string(&rules).unwrap();
    assert!(json.contains(r#""board_shape":"hexagon""#));
    assert!(json.contains(r#""premium_policy":"main_word_only""#));
    assert!(!json.contains("main_word_length")); // default omitted
    let reconstructed: Ruleset = serde_json::from_str(&json).unwrap();
    assert_eq!(reconstructed, rules);
    let mut letters = rules.clone();
    letters.main_word_length = WordLengthMetric::Letters;
    let json = serde_json::to_string(&letters).unwrap();
    assert!(json.contains(r#""main_word_length":"letters""#));
    assert_eq!(serde_json::from_str::<Ruleset>(&json).unwrap(), letters);
    let mut unknown: serde_json::Value = serde_json::to_value(&rules).unwrap();
    unknown["board_shape"] = "octagon".into();
    assert!(serde_json::from_value::<Ruleset>(unknown).is_err());

    let seed = (0..=255)
        .find(|&seed| {
            let session = new_session(&reconstructed, seed);
            count(session.state(), 1, 'A') >= 2 && count(session.state(), 2, 'A') >= 1
        })
        .expect("deterministic hex session fixture");
    let mut original = new_session(&reconstructed, seed);
    let log = play(&mut original);

    let snapshot = original.public_snapshot();
    assert_eq!(snapshot.configuration, rules);
    assert_eq!(snapshot.configuration.board_shape, BoardShape::Hexagon);
    assert_eq!(snapshot.configuration.premiums.len(), 37);
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();
    assert!(snapshot_json.contains(r#""board_shape":"hexagon""#));
    let mut board: Vec<_> = snapshot
        .board
        .iter()
        .map(|tile| (tile.coordinate.row(), tile.coordinate.column()))
        .collect();
    board.sort_unstable();
    assert_eq!(board, [(2, 4), (3, 3), (4, 3)]);
    // Opening: (1 + 1) · 2 = 4 on the double word; reply Vertical (3,3)(4,3): 1 + 1 = 2.
    assert_eq!(
        snapshot
            .players
            .iter()
            .map(|player| player.score)
            .collect::<Vec<_>>(),
        [4, 2]
    );
    assert_eq!(
        snapshot.phase,
        Phase::Playing {
            active_player: PlayerId::new(2),
            turn: TurnId::new(3)
        }
    );

    let canonical = original.state().canonical_bytes().unwrap();
    let evidence: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
    assert_eq!(evidence["ruleset"]["board_shape"], "hexagon");
    let from_evidence: Ruleset = serde_json::from_value(evidence["ruleset"].clone()).unwrap();
    assert_eq!(from_evidence, rules);

    // Replay the accepted commands on a match reconstructed from the canonical ruleset.
    let mut replay = new_session(&from_evidence, seed);
    for (player, sequence, command) in &log {
        replay
            .apply(PlayerId::new(*player), *sequence, command)
            .unwrap();
    }
    assert_eq!(replay.state().canonical_bytes().unwrap(), canonical);
    assert_eq!(replay.public_snapshot(), snapshot);
    replay.state().verify_tile_conservation().unwrap();
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Lengths and FNV-1a hashes were captured on the pre-#110 baseline 0a4daa0.
#[test]
fn square_rulesets_states_and_snapshots_stay_byte_identical() {
    for bytes in [
        serde_json::to_vec(&english_fixture()).unwrap(),
        serde_json::to_vec(&german_fixture()).unwrap(),
    ] {
        let text = String::from_utf8(bytes.clone()).unwrap();
        for key in ["board_shape", "premium_policy", "main_word_length"] {
            assert!(!text.contains(key), "square JSON gained {key}");
        }
        let parsed: Ruleset = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.board_shape, BoardShape::Square);
        assert_eq!(parsed.premium_policy, PremiumPolicy::AllWord);
        assert_eq!(parsed.main_word_length, WordLengthMetric::Tiles);
    }
    let english = serde_json::to_vec(&english_fixture()).unwrap();
    assert_eq!(
        (english.len(), fnv1a(&english)),
        (4293, 0x79e3_f8d6_9ef3_c050)
    );
    let german = serde_json::to_vec(&german_fixture()).unwrap();
    assert_eq!(
        (german.len(), fnv1a(&german)),
        (1403, 0xab55_aea4_ccde_0bd4)
    );

    let mut state = GameState::new(GameId::new(1), english_fixture(), [7; 32]).unwrap();
    state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    state.deal_initial_racks().unwrap();
    let rack = state.rack(PlayerId::new(1)).unwrap();
    let placement = |index: usize, row, column| Placement {
        tile_id: rack[index].id(),
        coordinate: Coordinate::new(row, column).unwrap(),
        blank_as: matches!(rack[index].face(), TileFace::Blank {}).then_some('A'),
    };
    // The square board never treats the hex Diagonal step (+1, -1) as a word axis.
    assert_eq!(
        validate(
            &state,
            PlayerId::new(1),
            &[placement(0, 7, 8), placement(1, 8, 7)]
        )
        .err(),
        Some(PlacementError::NotLine)
    );
    let placements = [placement(0, 7, 7), placement(1, 7, 8)];
    let result = commit_placement(&mut state, PlayerId::new(1), &placements).unwrap();
    assert_eq!(result.word_scores(), [4]);
    assert_eq!(result.main_word().direction(), Direction::Horizontal);
    assert_eq!(result.main_word().score(), 4);
    let canonical = state.canonical_bytes().unwrap();
    assert_eq!(
        (canonical.len(), fnv1a(&canonical)),
        (11704, 0x12c1_ef89_0fdb_ffe1)
    );
    let snapshot = serde_json::to_vec(&state.public_snapshot()).unwrap();
    assert_eq!(
        (snapshot.len(), fnv1a(&snapshot)),
        (4847, 0x89fc_1cd4_4238_0b7a)
    );
}
