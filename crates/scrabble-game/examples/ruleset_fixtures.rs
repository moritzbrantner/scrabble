//! Generated browser evidence from the same Unicode ruleset and authoritative command boundary.
use scrabble_game::{
    dictionary::WordList,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Placement},
    ruleset::{TileFace, german_fixture},
    session::GameSession,
    state::GameState,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn fixtures() -> Result<Value, Box<dyn std::error::Error>> {
    let rules = german_fixture();
    let player = PlayerId::new(1);
    let mut selected = None;
    for seed in 0..=255 {
        let mut state = GameState::new(GameId::new(2), rules.clone(), [seed; 32])?;
        state.add_player(player, "Ada".into())?;
        state.add_player(PlayerId::new(2), "Lin".into())?;
        state.deal_initial_racks()?;
        let rack = state.rack(player).ok_or("missing rack")?;
        if rack.iter().any(|tile| tile.face() == (TileFace::Blank {}))
            && rack
                .iter()
                .any(|tile| tile.face() == (TileFace::Letter { letter: 'L' }))
        {
            selected = Some(state);
            break;
        }
    }
    let state = selected.ok_or("missing authored blank/letter deal")?;
    let playing = state.player_snapshot(player)?;
    let rack = state.rack(player).ok_or("missing rack")?;
    let blank = rack
        .iter()
        .find(|tile| tile.face() == (TileFace::Blank {}))
        .ok_or("missing blank")?;
    let letter = rack
        .iter()
        .find(|tile| tile.face() == (TileFace::Letter { letter: 'L' }))
        .ok_or("missing L")?;
    let placements = vec![
        Placement {
            tile_id: blank.id(),
            coordinate: Coordinate::new(3, 3)?,
            blank_as: Some('Ö'),
        },
        Placement {
            tile_id: letter.id(),
            coordinate: Coordinate::new(3, 4)?,
            blank_as: None,
        },
    ];
    let dictionary = Arc::new(WordList::from_text(
        rules.dictionary.clone(),
        include_str!("../fixtures/de-words.txt"),
    )?);
    let mut session = GameSession::with_dictionary(state, dictionary);
    session.apply(
        player,
        1,
        &CommandEnvelope {
            version: ProtocolVersion,
            game_id: GameId::new(2),
            player_id: player,
            sequence: 1,
            expected_turn: TurnId::new(0),
            command: Command::Commit { placements },
        },
    )?;
    let committed = session.player_snapshot(player)?;
    assert_eq!(committed.public.players[0].score, 8);
    Ok(json!({"ruleset":rules,"playing":playing,"committed":committed}))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string_pretty(&fixtures()?)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn browser_variant_fixture_matches_real_blank_scoring_and_authoritative_commit() {
        let actual: serde_json::Value = serde_json::from_str(include_str!(
            "../../../apps/web/src/fixtures/ruleset-variant.json"
        ))
        .unwrap();
        assert_eq!(actual, super::fixtures().unwrap());
    }
}
