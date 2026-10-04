use scrabble_game::identity::{Coordinate, GameId, PlayerId, ProtocolVersion};
use scrabble_game::protocol::{Command, CommandEnvelope, Phase, Placement};
use scrabble_game::ruleset::{TileFace, english_fixture};
use scrabble_game::scoring::commit_placement;
use scrabble_game::session::GameSession;
use scrabble_game::state::GameState;
use serde_json::{Value, json};

fn fixtures() -> Result<Value, Box<dyn std::error::Error>> {
    let rules = english_fixture();
    let mut state = GameState::new(GameId::new(1), rules.clone(), [7; 32])?;
    let lobby = state.public_snapshot();
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
    let rack = state.rack(PlayerId::new(1)).ok_or("missing fixture rack")?;
    let blank = rack
        .iter()
        .find(|tile| matches!(tile.face(), TileFace::Blank {}))
        .ok_or("missing fixture blank")?;
    let letter = rack
        .iter()
        .find(|tile| matches!(tile.face(), TileFace::Letter { .. }))
        .ok_or("missing fixture letter")?;
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
    commit_placement(&mut state, PlayerId::new(1), &placements)?;
    let playing = state.public_snapshot();
    let mut session = GameSession::new(state);
    for sequence in 1..=rules.scoreless_turn_limit {
        let Phase::Playing {
            active_player,
            turn,
        } = *session.state().phase()
        else {
            return Err("fixture finished early".into());
        };
        let sequence = u32::from(sequence);
        session.apply(
            active_player,
            sequence,
            &CommandEnvelope {
                version: ProtocolVersion,
                game_id: GameId::new(1),
                player_id: active_player,
                sequence,
                expected_turn: turn,
                command: Command::Pass {},
            },
        )?;
    }
    let finished = session.public_snapshot();
    Ok(json!({"ruleset":rules,"snapshots":{"lobby":lobby,"playing":playing,"finished":finished}}))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string_pretty(&fixtures()?)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn committed_browser_fixture_matches_the_rust_public_contract() {
        let expected = super::fixtures().unwrap();
        let actual: serde_json::Value =
            serde_json::from_str(include_str!("../../../apps/web/src/fixtures/public.json"))
                .unwrap();
        assert_eq!(actual, expected);
    }
}
