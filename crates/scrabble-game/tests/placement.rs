use scrabble_game::identity::{Coordinate, GameId, PlayerId};
use scrabble_game::placement::{Direction, PlacementError, validate};
use scrabble_game::protocol::Placement;
use scrabble_game::ruleset::{TileDefinition, TileFace, english_fixture};
use scrabble_game::state::{GameState, StateError};

fn fixture(blanks: bool) -> GameState {
    let mut rules = english_fixture();
    rules.identity.name = "structural-test".into();
    rules.tiles = vec![TileDefinition {
        face: TileFace::Letter { letter: 'A' },
        count: if blanks { 50 } else { 100 },
        value: 1,
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
fn seed_board(state: &mut GameState, coords: &[(u8, u8)]) {
    let tiles = draft(state, 2, coords);
    state.place_tiles(PlayerId::new(2), &tiles).unwrap();
    state.refill_rack(PlayerId::new(2)).unwrap();
}
fn rejected(state: &GameState, coords: &[(u8, u8)], error: PlacementError) {
    let before = state.canonical_bytes().unwrap();
    assert!(
        matches!(validate(state,PlayerId::new(1),&draft(state,1,coords)),Err(actual) if actual==error)
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
}

#[test]
fn first_move_covers_center_and_has_a_word() {
    let state = fixture(false);
    rejected(&state, &[], PlacementError::Empty);
    rejected(&state, &[(0, 0), (0, 1)], PlacementError::CenterRequired);
    rejected(&state, &[(7, 7)], PlacementError::NoWord);
    let before = state.canonical_bytes().unwrap();
    let result = validate(
        &state,
        PlayerId::new(1),
        &draft(&state, 1, &[(7, 8), (7, 7)]),
    )
    .unwrap();
    assert_eq!(result.words()[0].text(), "AA");
    assert_eq!(result.placements()[0].coordinate.column(), 7);
    assert!(result.words()[0].tiles().iter().all(|tile| tile.is_new()));
    assert_eq!(state.canonical_bytes().unwrap(), before);
}

#[test]
fn diagonal_gaps_and_disconnected_parallel_moves_are_rejected() {
    let mut state = fixture(false);
    rejected(&state, &[(7, 7), (8, 8)], PlacementError::NotLine);
    rejected(&state, &[(7, 6), (7, 8)], PlacementError::Gap);
    seed_board(&mut state, &[(7, 7)]);
    rejected(&state, &[(0, 0), (0, 1)], PlacementError::Disconnected);
    rejected(&state, &[(5, 6), (5, 7)], PlacementError::Disconnected);
    rejected(
        &state,
        &[(7, 7)],
        PlacementError::Storage(StateError::OccupiedSquare),
    );
}

#[test]
fn existing_tiles_can_fill_gaps_and_extend_both_axes() {
    let mut state = fixture(false);
    seed_board(&mut state, &[(7, 7)]);
    let result = validate(
        &state,
        PlayerId::new(1),
        &draft(&state, 1, &[(7, 6), (7, 8)]),
    )
    .unwrap();
    assert_eq!(result.words()[0].text(), "AAA");
    assert!(!result.words()[0].tiles()[1].is_new());
    let mut state = fixture(false);
    seed_board(&mut state, &[(7, 6), (7, 7)]);
    let result = validate(&state, PlayerId::new(1), &draft(&state, 1, &[(7, 8)])).unwrap();
    assert_eq!(result.words()[0].text(), "AAA");
    assert_eq!(result.words()[0].direction(), Direction::Horizontal);
    let mut state = fixture(false);
    seed_board(&mut state, &[(6, 7), (7, 7)]);
    let result = validate(&state, PlayerId::new(1), &draft(&state, 1, &[(8, 7)])).unwrap();
    assert_eq!(result.words()[0].text(), "AAA");
    assert_eq!(result.words()[0].direction(), Direction::Vertical);
}

#[test]
fn crossing_and_parallel_connected_moves_derive_every_word_once() {
    let mut state = fixture(false);
    seed_board(&mut state, &[(6, 7), (8, 7)]);
    let result = validate(
        &state,
        PlayerId::new(1),
        &draft(&state, 1, &[(7, 6), (7, 7), (7, 8)]),
    )
    .unwrap();
    assert_eq!(
        result
            .words()
            .iter()
            .map(|word| word.text())
            .collect::<Vec<_>>(),
        vec!["AAA", "AAA"]
    );
    assert_eq!(result.words()[1].direction(), Direction::Vertical);
    let mut state = fixture(false);
    seed_board(&mut state, &[(7, 6), (7, 7), (7, 8)]);
    let placements = draft(&state, 1, &[(6, 8), (6, 6), (6, 7)]);
    let result = validate(&state, PlayerId::new(1), &placements).unwrap();
    assert_eq!(
        result
            .words()
            .iter()
            .map(|word| word.text())
            .collect::<Vec<_>>(),
        vec!["AAA", "AA", "AA", "AA"]
    );
    assert_eq!(result.words()[1].tiles()[0].coordinate().column(), 6);
    let mut reversed = placements.clone();
    reversed.reverse();
    assert_eq!(
        result.words(),
        validate(&state, PlayerId::new(1), &reversed)
            .unwrap()
            .words()
    );
}

#[test]
fn single_tile_at_two_word_intersection_is_not_counted_twice_per_axis() {
    let mut state = fixture(false);
    seed_board(&mut state, &[(7, 6), (6, 7)]);
    let result = validate(&state, PlayerId::new(1), &draft(&state, 1, &[(7, 7)])).unwrap();
    assert_eq!(result.words().len(), 2);
    assert_eq!(result.words()[0].direction(), Direction::Horizontal);
    assert_eq!(result.words()[1].direction(), Direction::Vertical);
}

#[test]
fn blank_assignment_retains_zero_value_and_rejects_invalid_letters() {
    let state = fixture(true);
    let rack = state.rack(PlayerId::new(1)).unwrap();
    let blank = rack
        .iter()
        .find(|tile| matches!(tile.face(), TileFace::Blank {}))
        .unwrap();
    let other = rack.iter().find(|tile| tile.id() != blank.id()).unwrap();
    let mut placements = vec![
        Placement {
            tile_id: blank.id(),
            coordinate: Coordinate::new(7, 7).unwrap(),
            blank_as: Some('A'),
        },
        Placement {
            tile_id: other.id(),
            coordinate: Coordinate::new(7, 8).unwrap(),
            blank_as: if matches!(other.face(), TileFace::Blank {}) {
                Some('A')
            } else {
                None
            },
        },
    ];
    let result = validate(&state, PlayerId::new(1), &placements).unwrap();
    assert_eq!(result.words()[0].tiles()[0].value(), 0);
    assert!(result.words()[0].tiles()[0].is_blank());
    placements[0].blank_as = Some('Z');
    assert!(matches!(
        validate(&state, PlayerId::new(1), &placements),
        Err(PlacementError::Storage(StateError::InvalidBlankAssignment))
    ));
}

#[test]
fn distinct_letters_and_values_follow_board_order_in_main_and_cross_words() {
    let mut rules = english_fixture();
    rules.identity.name = "word-order-test".into();
    rules.rack_size = 15;
    rules.maximum_players = 2;
    rules.tiles = vec![
        TileDefinition {
            face: TileFace::Letter { letter: 'A' },
            count: 10,
            value: 1,
        },
        TileDefinition {
            face: TileFace::Letter { letter: 'C' },
            count: 10,
            value: 3,
        },
        TileDefinition {
            face: TileFace::Letter { letter: 'T' },
            count: 10,
            value: 1,
        },
    ];
    let mut state = GameState::new(GameId::new(1), rules, [7; 32]).unwrap();
    state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    state.deal_initial_racks().unwrap();
    let rack = state.rack(PlayerId::new(2)).unwrap();
    let cs: Vec<_> = rack
        .iter()
        .filter(|tile| tile.face() == TileFace::Letter { letter: 'C' })
        .take(2)
        .map(|tile| tile.id())
        .collect();
    let t = rack
        .iter()
        .find(|tile| tile.face() == TileFace::Letter { letter: 'T' })
        .unwrap()
        .id();
    let seed = vec![
        Placement {
            tile_id: cs[0],
            coordinate: Coordinate::new(7, 6).unwrap(),
            blank_as: None,
        },
        Placement {
            tile_id: t,
            coordinate: Coordinate::new(6, 7).unwrap(),
            blank_as: None,
        },
        Placement {
            tile_id: cs[1],
            coordinate: Coordinate::new(8, 7).unwrap(),
            blank_as: None,
        },
    ];
    state.place_tiles(PlayerId::new(2), &seed).unwrap();
    let rack = state.rack(PlayerId::new(1)).unwrap();
    let a = rack
        .iter()
        .find(|tile| tile.face() == TileFace::Letter { letter: 'A' })
        .unwrap()
        .id();
    let t = rack
        .iter()
        .find(|tile| tile.face() == TileFace::Letter { letter: 'T' })
        .unwrap()
        .id();
    let proposed = vec![
        Placement {
            tile_id: t,
            coordinate: Coordinate::new(7, 8).unwrap(),
            blank_as: None,
        },
        Placement {
            tile_id: a,
            coordinate: Coordinate::new(7, 7).unwrap(),
            blank_as: None,
        },
    ];
    let before = state.canonical_bytes().unwrap();
    let result = validate(&state, PlayerId::new(1), &proposed).unwrap();
    assert_eq!(
        result
            .words()
            .iter()
            .map(|word| word.text())
            .collect::<Vec<_>>(),
        vec!["CAT", "TAC"]
    );
    assert_eq!(
        result.words()[0]
            .tiles()
            .iter()
            .map(|tile| tile.value())
            .collect::<Vec<_>>(),
        vec![3, 1, 1]
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
}
