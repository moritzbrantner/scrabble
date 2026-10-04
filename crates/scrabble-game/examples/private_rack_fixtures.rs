//! Test-only authoritative rack projections, including a committed placement and replacement draw.
use scrabble_game::{
    identity::{Coordinate, GameId, PlayerId},
    protocol::Placement,
    ruleset::{TileFace, english_fixture},
    scoring::commit_placement,
    state::GameState,
};
use serde_json::{Value, json};

fn fixtures() -> Result<Value, Box<dyn std::error::Error>> {
    let rules = english_fixture();
    let player = PlayerId::new(1);
    let mut state = GameState::new(GameId::new(1), rules.clone(), [0; 32])?;
    for seed in 0..=255 {
        state = GameState::new(GameId::new(1), rules.clone(), [seed; 32])?;
        state.add_player(player, "Ada".into())?;
        state.add_player(PlayerId::new(2), "Lin".into())?;
        state.deal_initial_racks()?;
        if state.rack(player).is_some_and(|rack| {
            rack.iter()
                .any(|tile| matches!(tile.face(), TileFace::Blank {}))
        }) {
            break;
        }
    }
    let playing = state.player_snapshot(player)?;
    let rack = state.rack(player).ok_or("missing rack")?;
    let blank = rack
        .iter()
        .find(|tile| matches!(tile.face(), TileFace::Blank {}))
        .ok_or("missing blank")?;
    let letter = rack
        .iter()
        .find(|tile| matches!(tile.face(), TileFace::Letter { .. }))
        .ok_or("missing letter")?;
    let placements = [
        Placement {
            tile_id: blank.id(),
            coordinate: Coordinate::new(7, 7)?,
            blank_as: Some('A'),
        },
        Placement {
            tile_id: letter.id(),
            coordinate: Coordinate::new(7, 8)?,
            blank_as: None,
        },
    ];
    commit_placement(&mut state, player, &placements)?;
    let committed = state.player_snapshot(player)?;
    assert_eq!(state.refill_rack(player)?, 2);
    state.verify_tile_conservation()?;
    let drawn = state.player_snapshot(player)?;
    let other_ids: Vec<_> = state
        .rack(PlayerId::new(2))
        .ok_or("missing other rack")?
        .iter()
        .map(|tile| tile.id())
        .collect();
    Ok(
        json!({ "ruleset": rules, "snapshots": { "playing": playing, "committed": committed, "drawn": drawn }, "otherTileIds": other_ids }),
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string_pretty(&fixtures()?)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn committed_private_rack_fixtures_match_authoritative_deal_commit_and_draw() {
        let actual: serde_json::Value = serde_json::from_str(include_str!(
            "../../../apps/web/src/fixtures/private-rack.json"
        ))
        .unwrap();
        assert_eq!(actual, super::fixtures().unwrap());
    }
}
