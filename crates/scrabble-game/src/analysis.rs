//! Non-mutating explanations of the same placement, dictionary and scoring authority as commit.
use crate::{
    dictionary::{self, Dictionary},
    identity::PlayerId,
    placement::{self, PlacementError},
    protocol::{EvaluationProblem, Placement, PlacementEvaluation},
    scoring,
    state::GameState,
};

pub fn evaluate(
    state: &GameState,
    player: PlayerId,
    placements: &[Placement],
    dictionary: &dyn Dictionary,
) -> PlacementEvaluation {
    let invalid = |problem| PlacementEvaluation::Invalid {
        problem,
        words: Vec::new(),
    };
    let validated = match placement::validate(state, player, placements) {
        Ok(value) => value,
        Err(error) => {
            return invalid(match error {
                PlacementError::CenterRequired => EvaluationProblem::Center,
                PlacementError::NotLine => EvaluationProblem::NotLine,
                PlacementError::Gap => EvaluationProblem::Gap,
                PlacementError::Disconnected => EvaluationProblem::Disconnected,
                PlacementError::NoWord | PlacementError::Empty => EvaluationProblem::NoWord,
                PlacementError::TooManyTiles | PlacementError::Storage(_) => {
                    EvaluationProblem::Unavailable
                }
            });
        }
    };
    if let Err(error) = dictionary::validate(&validated, dictionary) {
        return match error {
            dictionary::DictionaryError::InvalidWords { words } => PlacementEvaluation::Invalid {
                problem: EvaluationProblem::InvalidWords,
                words,
            },
            dictionary::DictionaryError::IdentityMismatch => invalid(EvaluationProblem::Dictionary),
        };
    }
    match scoring::calculate(&validated) {
        Ok(score) => PlacementEvaluation::Valid {
            analysis: score.analysis(state.ruleset().main_word_length),
        },
        Err(_) => invalid(EvaluationProblem::ScoreOverflow),
    }
}
