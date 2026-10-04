//! One atomic transaction for a validated, scored turn and deterministic replacement draws.
use crate::{
    dictionary::{self, Dictionary, DictionaryError},
    identity::PlayerId,
    placement::{self, PlacementError},
    protocol::{Phase, Placement},
    scoring::{self, MoveScore, ScoredCommitError},
    state::{GameState, StateError},
};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitError {
    NotActive,
    Placement(PlacementError),
    Dictionary(DictionaryError),
    Score(ScoredCommitError),
    State(StateError),
}
impl fmt::Display for CommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for CommitError {}

pub fn apply(
    state: &mut GameState,
    player: PlayerId,
    placements: &[Placement],
) -> Result<MoveScore, CommitError> {
    apply_with_dictionary(
        state,
        player,
        placements,
        dictionary::authored_fixture().as_ref(),
    )
}

pub fn apply_with_dictionary(
    state: &mut GameState,
    player: PlayerId,
    placements: &[Placement],
    dictionary: &dyn Dictionary,
) -> Result<MoveScore, CommitError> {
    if !matches!(state.phase(), Phase::Playing { active_player, .. } if *active_player == player) {
        return Err(CommitError::NotActive);
    }
    let validated =
        placement::validate(state, player, placements).map_err(CommitError::Placement)?;
    dictionary::validate(&validated, dictionary).map_err(CommitError::Dictionary)?;
    // Publish only after every mutation succeeds, including turn/revision overflow checks.
    let mut candidate = state.clone();
    let score = scoring::commit_placement(&mut candidate, player, placements)
        .map_err(CommitError::Score)?;
    candidate.refill_rack(player).map_err(CommitError::State)?;
    candidate.advance_turn().map_err(CommitError::State)?;
    *state = candidate;
    Ok(score)
}
