use scrabble_game::identity::{Coordinate, GameId, PlayerId};
use scrabble_game::placement::validate;
use scrabble_game::protocol::Placement;
use scrabble_game::ruleset::{Premium, TileDefinition, TileFace, english_fixture};
use scrabble_game::scoring::{ScoreError, ScoredCommitError, calculate, commit_placement};
use scrabble_game::state::GameState;

fn game(premiums: &[(u8, u8, Premium)], blanks: bool, value: u16, rack_size: u8) -> GameState {
    let mut rules = english_fixture();
    rules.identity.name = "scoring-test".into();
    rules.rack_size = rack_size;
    rules.premiums.fill(Premium::Normal);
    for &(row, column, premium) in premiums {
        rules.premiums[usize::from(row) * 15 + usize::from(column)] = premium;
    }
    rules.tiles = vec![TileDefinition {
        face: TileFace::Letter { letter: 'A' },
        count: if blanks { 50 } else { 100 },
        value,
    }];
    if blanks {
        rules.tiles.push(TileDefinition {
            face: TileFace::Blank {},
            count: 50,
            value: 0,
        });
    }
    let mut state = GameState::new(GameId::new(1), rules, [7; 32]).unwrap();
    state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    state.deal_initial_racks().unwrap();
    state
}
fn draft(state: &GameState, player: u64, coords: &[(u8, u8)]) -> Vec<Placement> {
    coords
        .iter()
        .zip(state.rack(PlayerId::new(player)).unwrap())
        .map(|(&(row, column), tile)| Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(row, column).unwrap(),
            blank_as: if matches!(tile.face(), TileFace::Blank {}) {
                Some('A')
            } else {
                None
            },
        })
        .collect()
}
fn seed(state: &mut GameState, coords: &[(u8, u8)]) {
    let placements = draft(state, 2, coords);
    state.place_tiles(PlayerId::new(2), &placements).unwrap();
}

#[test]
fn letter_multipliers_precede_stacked_word_multipliers_and_scoring_is_pure() {
    let state = game(
        &[
            (7, 7, Premium::DoubleLetter),
            (7, 8, Premium::TripleLetter),
            (7, 9, Premium::DoubleWord),
            (7, 10, Premium::TripleWord),
        ],
        false,
        2,
        7,
    );
    let placements = draft(&state, 1, &[(7, 7), (7, 8), (7, 9), (7, 10)]);
    let before = state.canonical_bytes().unwrap();
    let validated = validate(&state, PlayerId::new(1), &placements).unwrap();
    let result = calculate(&validated).unwrap();
    assert_eq!(result.word_scores(), &[84]); // (4+6+2+2)*2*3
    assert_eq!(result.bingo_bonus(), 0);
    assert_eq!(result.total(), 84);
    assert_eq!(calculate(&validated).unwrap(), result);
    assert_eq!(state.canonical_bytes().unwrap(), before);
}

#[test]
fn committed_premiums_are_spent_and_scores_change_only_on_successful_commit() {
    let mut state = game(&[(7, 7, Premium::DoubleWord)], false, 2, 7);
    let first = draft(&state, 1, &[(7, 7), (7, 8)]);
    assert_eq!(
        commit_placement(&mut state, PlayerId::new(1), &first)
            .unwrap()
            .total(),
        8
    );
    assert_eq!(state.public_snapshot().players[0].score, 8);
    let before = state.canonical_bytes().unwrap();
    assert!(commit_placement(&mut state, PlayerId::new(1), &first).is_err());
    assert_eq!(state.canonical_bytes().unwrap(), before);
    let extension = draft(&state, 1, &[(7, 9)]);
    let score = calculate(&validate(&state, PlayerId::new(1), &extension).unwrap()).unwrap();
    assert_eq!(score.total(), 6);
    assert_eq!(state.public_snapshot().players[0].score, 8);
    assert_eq!(
        commit_placement(&mut state, PlayerId::new(1), &extension).unwrap(),
        score
    );
    assert_eq!(state.public_snapshot().players[0].score, 14);
    state.verify_tile_conservation().unwrap();
}

#[test]
fn new_word_and_letter_premiums_apply_to_each_crossing_word() {
    let mut state = game(&[(7, 7, Premium::DoubleWord)], false, 2, 7);
    seed(&mut state, &[(6, 7), (8, 7)]);
    let placements = draft(&state, 1, &[(7, 6), (7, 7), (7, 8)]);
    let score = calculate(&validate(&state, PlayerId::new(1), &placements).unwrap()).unwrap();
    assert_eq!(score.word_scores(), &[12, 12]);
    assert_eq!(score.total(), 24);
    let mut state = game(&[(7, 7, Premium::TripleLetter)], false, 2, 7);
    seed(&mut state, &[(7, 6), (6, 7)]);
    let placements = draft(&state, 1, &[(7, 7)]);
    let score = calculate(&validate(&state, PlayerId::new(1), &placements).unwrap()).unwrap();
    assert_eq!(score.word_scores(), &[8, 8]);
    assert_eq!(score.total(), 16);
}

#[test]
fn blanks_have_zero_letter_value_but_still_activate_word_premiums() {
    for (premium, expected) in [(Premium::TripleLetter, 2), (Premium::TripleWord, 6)] {
        let mut state = game(&[(7, 7, premium)], true, 2, 7);
        // Seed a known ordinary tile to avoid accidental zero-value fixture scoring.
        let tile = state
            .rack(PlayerId::new(2))
            .unwrap()
            .iter()
            .find(|tile| matches!(tile.face(), TileFace::Letter { .. }))
            .unwrap();
        let old = Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(7, 6).unwrap(),
            blank_as: None,
        };
        state.place_tiles(PlayerId::new(2), &[old]).unwrap();
        let blank = state
            .rack(PlayerId::new(1))
            .unwrap()
            .iter()
            .find(|tile| matches!(tile.face(), TileFace::Blank {}))
            .unwrap();
        let placement = Placement {
            tile_id: blank.id(),
            coordinate: Coordinate::new(7, 7).unwrap(),
            blank_as: Some('A'),
        };
        assert_eq!(
            calculate(
                &validate(&state, PlayerId::new(1), std::slice::from_ref(&placement)).unwrap()
            )
            .unwrap()
            .total(),
            expected
        );
        commit_placement(&mut state, PlayerId::new(1), &[placement]).unwrap();
        state.verify_tile_conservation().unwrap();
        let next = state
            .rack(PlayerId::new(1))
            .unwrap()
            .iter()
            .find(|tile| matches!(tile.face(), TileFace::Letter { .. }))
            .unwrap();
        let extension = Placement {
            tile_id: next.id(),
            coordinate: Coordinate::new(7, 8).unwrap(),
            blank_as: None,
        };
        assert_eq!(
            calculate(&validate(&state, PlayerId::new(1), &[extension]).unwrap())
                .unwrap()
                .total(),
            4
        );
    }
}

#[test]
fn full_configured_rack_earns_one_bonus_after_all_words() {
    for rack_size in [3, 7] {
        let state = game(&[], false, 2, rack_size);
        let coords: Vec<_> = (0..rack_size).map(|offset| (7, 7 + offset)).collect();
        let placements = draft(&state, 1, &coords);
        let score = calculate(&validate(&state, PlayerId::new(1), &placements).unwrap()).unwrap();
        assert_eq!(score.bingo_bonus(), 50);
        assert_eq!(score.total(), 2 * u32::from(rack_size) + 50);
        let short = &placements[..placements.len() - 1];
        assert_eq!(
            calculate(&validate(&state, PlayerId::new(1), short).unwrap())
                .unwrap()
                .bingo_bonus(),
            0
        );
    }
    let mut state = game(&[], false, 2, 7);
    seed(
        &mut state,
        &[(7, 4), (7, 5), (7, 6), (7, 7), (7, 8), (7, 9), (7, 10)],
    );
    let placements = draft(
        &state,
        1,
        &[(6, 4), (6, 5), (6, 6), (6, 7), (6, 8), (6, 9), (6, 10)],
    );
    let score = calculate(&validate(&state, PlayerId::new(1), &placements).unwrap()).unwrap();
    assert_eq!(score.word_scores().len(), 8);
    assert_eq!(score.bingo_bonus(), 50);
    assert_eq!(score.total(), 92);
    let mut state = game(&[], false, 2, 7);
    let one = draft(&state, 1, &[(7, 7)]);
    state.place_tiles(PlayerId::new(1), &one).unwrap();
    let remaining = draft(
        &state,
        1,
        &[(7, 8), (7, 9), (7, 10), (7, 11), (7, 12), (7, 13)],
    );
    assert_eq!(remaining.len(), state.rack(PlayerId::new(1)).unwrap().len());
    assert_eq!(
        calculate(&validate(&state, PlayerId::new(1), &remaining).unwrap())
            .unwrap()
            .bingo_bonus(),
        0
    );
}

#[test]
fn score_overflow_and_non_active_commits_fail_without_mutation() {
    let premiums: Vec<_> = (0..15)
        .map(|column| (7, column, Premium::TripleWord))
        .collect();
    let mut state = game(&premiums, false, u16::MAX, 15);
    let coords: Vec<_> = (0..15).map(|column| (7, column)).collect();
    let placements = draft(&state, 1, &coords);
    let before = state.canonical_bytes().unwrap();
    assert_eq!(
        calculate(&validate(&state, PlayerId::new(1), &placements).unwrap()),
        Err(ScoreError::Overflow)
    );
    assert_eq!(
        commit_placement(&mut state, PlayerId::new(1), &placements),
        Err(ScoredCommitError::Score(ScoreError::Overflow))
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
    let other = draft(&state, 2, &[(7, 7), (7, 8)]);
    assert_eq!(
        commit_placement(&mut state, PlayerId::new(2), &other),
        Err(ScoredCommitError::NotActive)
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
}

#[test]
fn all_occupied_premiums_are_ignored_on_extensions() {
    for premium in [
        Premium::DoubleLetter,
        Premium::TripleLetter,
        Premium::DoubleWord,
        Premium::TripleWord,
    ] {
        let mut state = game(&[(7, 7, premium)], false, 2, 7);
        seed(&mut state, &[(7, 7), (7, 8)]);
        let extension = draft(&state, 1, &[(7, 9)]);
        assert_eq!(
            calculate(&validate(&state, PlayerId::new(1), &extension).unwrap())
                .unwrap()
                .total(),
            6
        );
    }
}

#[test]
fn cumulative_score_range_is_checked_before_tile_transfer() {
    let premiums: Vec<_> = (0..8)
        .map(|column| (7, column, Premium::TripleWord))
        .collect();
    let mut state = game(&premiums, false, u16::MAX, 15);
    let coords: Vec<_> = (0..8).map(|column| (7, column)).collect();
    let placements = draft(&state, 1, &coords);
    let before = state.canonical_bytes().unwrap();
    let score = calculate(&validate(&state, PlayerId::new(1), &placements).unwrap()).unwrap();
    assert!(score.total() > i32::MAX as u32);
    assert_eq!(
        commit_placement(&mut state, PlayerId::new(1), &placements),
        Err(ScoredCommitError::CumulativeOverflow)
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
}
