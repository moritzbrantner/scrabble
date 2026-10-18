//! Retrospective #96 admission regression, authored after preview implementation.
//! Real legal deals/commands, no injected scores, boards or public projections.
use scrabble_game::{
    commit::CommitError,
    dictionary::WordList,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, EvaluationProblem, Placement, PlacementEvaluation},
    ruleset::{BoardShape, Premium, TileDefinition, TileFace, english_fixture},
    scoring::ScoredCommitError,
    session::{CommandError, GameSession},
    state::GameState,
};
use std::sync::Arc;
fn command(
    s: &mut GameSession,
    actor: u64,
    sequence: u32,
    turn: u64,
    command: Command,
) -> Result<(), CommandError> {
    s.apply(
        PlayerId::new(actor),
        sequence,
        &CommandEnvelope {
            version: ProtocolVersion,
            game_id: GameId::new(1),
            player_id: PlayerId::new(actor),
            sequence,
            expected_turn: TurnId::new(turn),
            command,
        },
    )
}
fn game(triples_per_row: u8) -> GameSession {
    let mut rules = english_fixture();
    rules.board_shape = BoardShape::Hexagon;
    rules.board_size = 15;
    rules.rack_size = 8;
    rules.bingo_bonus = 0;
    rules.tiles = vec![TileDefinition {
        face: TileFace::Letter { letter: 'A' },
        count: 200,
        value: u16::MAX,
    }];
    rules.premiums = (0..15)
        .flat_map(|row| {
            (0..15).filter_map(move |column| {
                if (7..=21).contains(&(row + column)) {
                    Some(if (row == 7 || row == 8) && column < triples_per_row {
                        Premium::TripleWord
                    } else {
                        Premium::Normal
                    })
                } else {
                    None
                }
            })
        })
        .collect();
    rules.validate().unwrap();
    let dictionary =
        Arc::new(WordList::from_text(rules.dictionary.clone(), "AA\nAAAAAAAA\n").unwrap());
    let mut session = GameSession::with_dictionary(
        GameState::new(GameId::new(1), rules, [7; 32]).unwrap(),
        dictionary,
    );
    for actor in 1..=2 {
        session
            .add_player(PlayerId::new(actor), format!("Player{actor}"))
            .unwrap();
    }
    command(&mut session, 1, 1, 0, Command::Start {}).unwrap();
    session
}
fn row(session: &GameSession, r: u8) -> Vec<Placement> {
    session
        .state()
        .rack(PlayerId::new(1))
        .unwrap()
        .iter()
        .enumerate()
        .map(|(column, tile)| Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(r, column as u8).unwrap(),
            blank_as: None,
        })
        .collect()
}
fn assert_preview_rejects_the_same_score_as_commit(
    mut s: GameSession,
    placements: Vec<Placement>,
    sequence: u32,
    turn: u64,
) {
    let before = s.state().canonical_bytes().unwrap();
    command(
        &mut s,
        1,
        sequence,
        turn,
        Command::Preview {
            placements: placements.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        s.state().canonical_bytes().unwrap(),
        before,
        "preview cannot mutate authority"
    );
    let evaluation = s
        .player_snapshot(PlayerId::new(1))
        .unwrap()
        .own_preview
        .unwrap()
        .evaluation
        .unwrap();
    let rejected = command(
        &mut s,
        1,
        sequence + 1,
        turn,
        Command::Commit { placements },
    );
    assert!(matches!(
        rejected,
        Err(CommandError::Commit(CommitError::Score(
            ScoredCommitError::CumulativeOverflow
        )))
    ));
    assert_eq!(
        s.state().canonical_bytes().unwrap(),
        before,
        "rejected commit must remain atomic"
    );
    assert!(
        matches!(
            evaluation,
            PlacementEvaluation::Invalid {
                problem: EvaluationProblem::ScoreOverflow,
                ..
            }
        ),
        "preview must not advertise a score the same native commit cannot admit"
    );
}
#[test]
fn hex_preview_rejects_a_move_total_outside_signed_score_capacity() {
    let session = game(8);
    let placements = row(&session, 7);
    let total = 8u64 * u64::from(u16::MAX) * 3u64.pow(8);
    assert!(total > i32::MAX as u64 && total <= u32::MAX as u64);
    assert_preview_rejects_the_same_score_as_commit(session, placements, 2, 0);
}
#[test]
fn hex_preview_rejects_cumulative_overflow_after_real_accepted_play() {
    let mut session = game(7);
    let opening = row(&session, 7);
    command(
        &mut session,
        1,
        2,
        0,
        Command::Commit {
            placements: opening,
        },
    )
    .unwrap();
    assert_eq!(
        session
            .public_snapshot()
            .players
            .iter()
            .find(|player| player.id == PlayerId::new(1))
            .map(|player| player.score),
        Some(1_146_600_360)
    );
    command(&mut session, 2, 1, 1, Command::Pass {}).unwrap();
    let second = row(&session, 8);
    assert_preview_rejects_the_same_score_as_commit(session, second, 3, 2);
}
