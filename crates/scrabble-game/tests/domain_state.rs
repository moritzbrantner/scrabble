use scrabble_game::identity::{Coordinate, GameId, PlayerId, TileId};
use scrabble_game::protocol::Placement;
use scrabble_game::ruleset::{TileFace, english_fixture};
use scrabble_game::state::{GameState, StateError};

fn game(seed: u8) -> GameState {
    let mut game = GameState::new(GameId::new(1), english_fixture(), [seed; 32]).unwrap();
    game.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    game.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    game.deal_initial_racks().unwrap();
    game
}
fn placements(game: &GameState, player: PlayerId, row: u8) -> Vec<Placement> {
    game.rack(player)
        .unwrap()
        .iter()
        .enumerate()
        .map(|(column, tile)| Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(row, column as u8).unwrap(),
            blank_as: match tile.face() {
                TileFace::Blank {} => Some('A'),
                TileFace::Letter { .. } => None,
            },
        })
        .collect()
}

#[test]
fn same_seed_and_ruleset_reproduce_every_draw_and_preserve_tiles() {
    let mut first = game(7);
    let mut second = game(7);
    for row in 0..15 {
        let player = PlayerId::new(1 + u64::from(row % 2));
        assert_eq!(first.rack(player), second.rack(player));
        let draft = placements(&first, player, row);
        first.place_tiles(player, &draft).unwrap();
        second.place_tiles(player, &draft).unwrap();
        assert_eq!(
            first.refill_rack(player).unwrap(),
            second.refill_rack(player).unwrap()
        );
        assert!(first.verify_tile_conservation().is_ok());
        assert!(second.verify_tile_conservation().is_ok());
    }
    assert_eq!(first.public_snapshot(), second.public_snapshot());
    assert_eq!(first.remaining_tiles(), 0);
    assert_eq!(first.refill_rack(PlayerId::new(1)).unwrap(), 0);
}

#[test]
fn rack_capacity_and_roster_are_enforced() {
    let mut state = GameState::new(GameId::new(1), english_fixture(), [0; 32]).unwrap();
    assert_eq!(
        state.deal_initial_racks(),
        Err(StateError::NotEnoughPlayers)
    );
    state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    assert_eq!(
        state.add_player(PlayerId::new(1), "Other".into()),
        Err(StateError::DuplicatePlayer)
    );
    assert_eq!(
        state.add_player(PlayerId::new(2), " ".into()),
        Err(StateError::InvalidName)
    );
    state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    state.deal_initial_racks().unwrap();
    assert_eq!(state.rack(PlayerId::new(1)).unwrap().len(), 7);
    assert_eq!(state.refill_rack(PlayerId::new(1)).unwrap(), 0);
    assert_eq!(state.remaining_tiles(), 86);
    assert_eq!(state.deal_initial_racks(), Err(StateError::WrongPhase));
    assert_eq!(
        state.add_player(PlayerId::new(3), "New".into()),
        Err(StateError::WrongPhase)
    );
}

#[test]
fn storage_rejects_forged_duplicate_or_occupied_placements_atomically() {
    let mut state = game(1);
    let player = PlayerId::new(1);
    let draft = placements(&state, player, 7);
    let rack = state.rack(player).unwrap().to_vec();
    let before = state.public_snapshot();
    let mut invalid = draft.clone();
    invalid[1].tile_id = invalid[0].tile_id;
    assert_eq!(
        state.place_tiles(player, &invalid),
        Err(StateError::DuplicateTile)
    );
    invalid = draft.clone();
    invalid[1].coordinate = invalid[0].coordinate;
    assert_eq!(
        state.place_tiles(player, &invalid),
        Err(StateError::DuplicateCoordinate)
    );
    invalid = draft.clone();
    invalid[0].tile_id = TileId::new(u64::MAX);
    assert_eq!(
        state.place_tiles(player, &invalid),
        Err(StateError::TileNotOwned)
    );
    assert_eq!(state.rack(player).unwrap(), rack);
    assert_eq!(state.public_snapshot(), before);
    state.place_tiles(player, &draft[..1]).unwrap();
    let mut collision = draft[1].clone();
    collision.coordinate = draft[0].coordinate;
    assert_eq!(
        state.place_tiles(player, &[collision]),
        Err(StateError::OccupiedSquare)
    );
    assert!(state.verify_tile_conservation().is_ok());
}

#[test]
fn blanks_keep_zero_value_and_require_an_alphabet_assignment() {
    let mut rules = english_fixture();
    rules.tiles.retain(|tile| {
        matches!(
            tile.face,
            TileFace::Blank {} | TileFace::Letter { letter: 'A' }
        )
    });
    rules.tiles.iter_mut().for_each(|tile| tile.count = 30);
    let mut state = GameState::new(GameId::new(9), rules, [1; 32]).unwrap();
    state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    state.deal_initial_racks().unwrap();
    let player = PlayerId::new(1);
    let blank = state
        .rack(player)
        .unwrap()
        .iter()
        .find(|tile| matches!(tile.face(), TileFace::Blank {}))
        .unwrap();
    assert_eq!(blank.value(), 0);
    let mut placement = Placement {
        tile_id: blank.id(),
        coordinate: Coordinate::new(7, 7).unwrap(),
        blank_as: None,
    };
    assert_eq!(
        state.place_tiles(player, &[placement.clone()]),
        Err(StateError::InvalidBlankAssignment)
    );
    placement.blank_as = Some('Z');
    assert_eq!(
        state.place_tiles(player, &[placement.clone()]),
        Err(StateError::InvalidBlankAssignment)
    );
    placement.blank_as = Some('A');
    state.place_tiles(player, &[placement]).unwrap();
    let committed = state.board_tile(Coordinate::new(7, 7).unwrap()).unwrap();
    assert!(committed.is_blank());
    assert_eq!(committed.letter(), 'A');
    assert_eq!(committed.tile().value(), 0);
    assert!(state.verify_tile_conservation().is_ok());
}

#[test]
fn public_projection_never_contains_rack_faces_or_bag_order() {
    let state = game(9);
    let public = serde_json::to_value(state.public_snapshot()).unwrap();
    assert!(public.get("bag").is_none());
    assert!(public.get("racks").is_none());
    for player in public["players"].as_array().unwrap() {
        assert_eq!(player["rack_count"], 7);
        assert!(player.get("tiles").is_none());
        assert!(player.get("rack").is_none());
    }
    let own = state.player_snapshot(PlayerId::new(1)).unwrap();
    assert_eq!(own.own_rack.player_id, PlayerId::new(1));
    assert_eq!(own.own_rack.tiles.len(), 7);
    assert!(state.player_snapshot(PlayerId::new(99)).is_err());
}

#[test]
fn seed_seven_has_a_versioned_initial_deal() {
    let state = game(7);
    let first: Vec<_> = state
        .rack(PlayerId::new(1))
        .unwrap()
        .iter()
        .map(|tile| tile.id().get())
        .collect();
    let second: Vec<_> = state
        .rack(PlayerId::new(2))
        .unwrap()
        .iter()
        .map(|tile| tile.id().get())
        .collect();
    assert_eq!(first, vec![7, 48, 88, 60, 75, 12, 19]);
    assert_eq!(second, vec![27, 76, 87, 81, 14, 67, 51]);
}

#[test]
fn configured_board_and_rack_bounds_are_preserved() {
    let mut rules = english_fixture();
    rules.board_size = 3;
    rules.premiums = vec![scrabble_game::ruleset::Premium::Normal; 9];
    rules.rack_size = 15;
    let mut state = GameState::new(GameId::new(4), rules, [2; 32]).unwrap();
    for id in 1..=4 {
        state
            .add_player(PlayerId::new(id), format!("Player {id}"))
            .unwrap();
    }
    assert_eq!(
        state.add_player(PlayerId::new(5), "Fifth".into()),
        Err(StateError::PlayerLimit)
    );
    state.deal_initial_racks().unwrap();
    assert_eq!(state.rack(PlayerId::new(1)).unwrap().len(), 15);
    assert_eq!(state.remaining_tiles(), 40);
    let tile = state.rack(PlayerId::new(1)).unwrap()[0].id();
    assert_eq!(
        state.place_tiles(
            PlayerId::new(1),
            &[Placement {
                tile_id: tile,
                coordinate: Coordinate::new(3, 0).unwrap(),
                blank_as: None
            }]
        ),
        Err(StateError::OutsideBoard)
    );
    assert!(state.verify_tile_conservation().is_ok());
}
