//! Non-mutating explanations of the exact atomic transaction used by commit.
use crate::{
    commit::{self, CommitError},
    dictionary::{Dictionary, DictionaryError},
    identity::PlayerId,
    placement::PlacementError,
    protocol::{EvaluationProblem, Placement, PlacementEvaluation},
    scoring::ScoredCommitError,
    state::{GameState, StateError},
};

pub fn evaluate(
    state: &GameState,
    player: PlayerId,
    placements: &[Placement],
    dictionary: &dyn Dictionary,
) -> PlacementEvaluation {
    evaluate_at(state, player, placements, dictionary, None)
}

pub(crate) fn evaluate_at(
    state: &GameState,
    player: PlayerId,
    placements: &[Placement],
    dictionary: &dyn Dictionary,
    now_ms: Option<u64>,
) -> PlacementEvaluation {
    let error =
        match commit::prepare_with_dictionary_at(state, player, placements, dictionary, now_ms) {
            Ok((_, score)) => {
                return PlacementEvaluation::Valid {
                    analysis: score.analysis(state.ruleset().main_word_length),
                };
            }
            Err(error) => error,
        };
    let problem = match error {
        CommitError::Placement(error) | CommitError::Score(ScoredCommitError::Placement(error)) => {
            match error {
                PlacementError::CenterRequired => EvaluationProblem::Center,
                PlacementError::NotLine => EvaluationProblem::NotLine,
                PlacementError::Gap => EvaluationProblem::Gap,
                PlacementError::Disconnected => EvaluationProblem::Disconnected,
                PlacementError::NoWord | PlacementError::Empty => EvaluationProblem::NoWord,
                PlacementError::TooManyTiles | PlacementError::Storage(_) => {
                    EvaluationProblem::Unavailable
                }
            }
        }
        CommitError::Dictionary(DictionaryError::InvalidWords { words }) => {
            return PlacementEvaluation::Invalid {
                problem: EvaluationProblem::InvalidWords,
                words,
            };
        }
        CommitError::Dictionary(DictionaryError::IdentityMismatch) => EvaluationProblem::Dictionary,
        CommitError::Score(ScoredCommitError::Score(_) | ScoredCommitError::CumulativeOverflow)
        | CommitError::State(StateError::ScoreOverflow)
        | CommitError::Score(ScoredCommitError::State(StateError::ScoreOverflow)) => {
            EvaluationProblem::ScoreOverflow
        }
        CommitError::NotActive
        | CommitError::State(_)
        | CommitError::Score(ScoredCommitError::NotActive | ScoredCommitError::State(_)) => {
            EvaluationProblem::Unavailable
        }
    };
    PlacementEvaluation::Invalid {
        problem,
        words: Vec::new(),
    }
}
