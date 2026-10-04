//! Independent Rust structural oracle for browser draft guidance.
use scrabble_game::{
    identity::{Coordinate, GameId, PlayerId, TileId},
    placement::{PlacementError, validate},
    protocol::Placement,
    ruleset::{TileFace, english_fixture},
    state::{GameState, StateError},
};
use serde_json::{Value, json};
fn problem(error: PlacementError) -> &'static str {
    match error {
        PlacementError::Empty => "empty",
        PlacementError::TooManyTiles => "too-many",
        PlacementError::NotLine => "not-line",
        PlacementError::Gap => "gap",
        PlacementError::CenterRequired => "center",
        PlacementError::Disconnected => "disconnected",
        PlacementError::NoWord => "no-word",
        PlacementError::Storage(StateError::TileNotOwned) => "not-owned",
        PlacementError::Storage(StateError::DuplicateTile | StateError::DuplicateCoordinate) => {
            "duplicate"
        }
        PlacementError::Storage(StateError::OccupiedSquare) => "occupied",
        PlacementError::Storage(StateError::InvalidBlankAssignment) => "blank",
        PlacementError::Storage(StateError::OutsideBoard) => "coordinate",
        other => panic!("unhandled oracle error: {other}"),
    }
}
fn cases(state: &GameState) -> Result<Value, Box<dyn std::error::Error>> {
    let player = PlayerId::new(1);
    let snapshot = state.player_snapshot(player)?;
    let rack = state.rack(player).ok_or("missing rack")?;
    let letters: Vec<_> = rack
        .iter()
        .filter(|tile| matches!(tile.face(), TileFace::Letter { .. }))
        .collect();
    let tile = |index: usize, row: u8, column: u8| -> Placement {
        Placement {
            tile_id: letters[index].id(),
            coordinate: Coordinate::new(row, column).unwrap(),
            blank_as: None,
        }
    };
    let mut inputs = vec![vec![]];
    for row in 0..15 {
        for column in 0..15 {
            inputs.push(vec![tile(0, row, column)]);
            if column < 14 {
                inputs.push(vec![tile(0, row, column), tile(1, row, column + 1)]);
            }
            if row < 14 {
                inputs.push(vec![tile(0, row, column), tile(1, row + 1, column)]);
            }
        }
    }
    inputs.extend([
        vec![tile(0, 7, 7), tile(1, 7, 9)],
        vec![tile(0, 7, 7), tile(1, 9, 7)],
        vec![tile(0, 7, 7), tile(1, 8, 8)],
        vec![tile(0, 7, 7), tile(0, 7, 8)],
        vec![tile(0, 7, 7), tile(1, 7, 7)],
        vec![tile(0, 7, 7); 8],
        vec![Placement {
            tile_id: TileId::new(99999),
            ..tile(0, 7, 7)
        }],
        vec![Placement {
            blank_as: Some('A'),
            ..tile(0, 7, 7)
        }],
    ]);
    if let Some(blank) = rack
        .iter()
        .find(|tile| matches!(tile.face(), TileFace::Blank {}))
    {
        for blank_as in [None, Some('A'), Some('a'), Some('!')] {
            inputs.push(vec![
                Placement {
                    tile_id: blank.id(),
                    coordinate: Coordinate::new(7, 7)?,
                    blank_as,
                },
                tile(0, 7, 8),
            ]);
        }
    }
    let results: Vec<_> = inputs
        .into_iter()
        .map(|placements| {
            let expected = validate(state, player, &placements).err().map(problem);
            json!({ "placements": placements, "expected": expected })
        })
        .collect();
    Ok(json!({ "ruleset": state.ruleset(), "snapshot": snapshot, "cases": results }))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rules = english_fixture();
    let mut state = GameState::new(GameId::new(1), rules.clone(), [0; 32])?;
    for seed in 0..=255 {
        state = GameState::new(GameId::new(1), rules.clone(), [seed; 32])?;
        state.add_player(PlayerId::new(1), "Ada".into())?;
        state.add_player(PlayerId::new(2), "Lin".into())?;
        state.deal_initial_racks()?;
        if state.rack(PlayerId::new(1)).is_some_and(|rack| {
            rack.iter()
                .any(|tile| matches!(tile.face(), TileFace::Blank {}))
        }) {
            break;
        }
    }
    let initial = cases(&state)?;
    let rack = state.rack(PlayerId::new(1)).ok_or("missing rack")?;
    let letters: Vec<_> = rack
        .iter()
        .filter(|tile| matches!(tile.face(), TileFace::Letter { .. }))
        .collect();
    let placements = [
        Placement {
            tile_id: letters[0].id(),
            coordinate: Coordinate::new(7, 7)?,
            blank_as: None,
        },
        Placement {
            tile_id: letters[1].id(),
            coordinate: Coordinate::new(7, 8)?,
            blank_as: None,
        },
    ];
    state.place_tiles(PlayerId::new(1), &placements)?;
    state.refill_rack(PlayerId::new(1))?;
    state.verify_tile_conservation()?;
    let mut compact_rules = rules.clone();
    compact_rules.board_size = 7;
    compact_rules.premiums = vec![scrabble_game::ruleset::Premium::Normal; 49];
    let mut compact = GameState::new(GameId::new(2), compact_rules, [7; 32])?;
    compact.add_player(PlayerId::new(1), "Ada".into())?;
    compact.add_player(PlayerId::new(2), "Lin".into())?;
    compact.deal_initial_racks()?;
    println!(
        "{}",
        json!({ "groups": [initial, cases(&state)?, cases(&compact)?] })
    );
    Ok(())
}
