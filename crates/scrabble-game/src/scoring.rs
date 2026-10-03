//! Pure scoring of structurally validated words, plus the storage-level scored commit.
use crate::identity::PlayerId;
use crate::placement::{self, PlacementError, ValidatedMove};
use crate::protocol::{Phase, Placement};
use crate::ruleset::Premium;
use crate::state::{GameState, StateError};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreError {
    Overflow,
}
impl fmt::Display for ScoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("score exceeds supported range")
    }
}
impl std::error::Error for ScoreError {}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveScore {
    word_scores: Vec<u32>,
    bingo_bonus: u32,
    total: u32,
}
impl MoveScore {
    pub fn word_scores(&self) -> &[u32] {
        &self.word_scores
    }
    pub const fn bingo_bonus(&self) -> u32 {
        self.bingo_bonus
    }
    pub const fn total(&self) -> u32 {
        self.total
    }
}

pub fn calculate(validated: &ValidatedMove<'_>) -> Result<MoveScore, ScoreError> {
    let rules = validated.ruleset();
    let mut word_scores = Vec::with_capacity(validated.words().len());
    let mut total = 0_u32;
    for word in validated.words() {
        let mut sum = 0_u32;
        let mut multiplier = 1_u32;
        for tile in word.tiles() {
            let coordinate = tile.coordinate();
            let premium = if tile.is_new() {
                rules.premiums[usize::from(coordinate.row()) * usize::from(rules.board_size)
                    + usize::from(coordinate.column())]
            } else {
                Premium::Normal
            };
            let (letter, word) = match premium {
                Premium::Normal => (1, 1),
                Premium::DoubleLetter => (2, 1),
                Premium::TripleLetter => (3, 1),
                Premium::DoubleWord => (1, 2),
                Premium::TripleWord => (1, 3),
            };
            let value = u32::from(tile.value())
                .checked_mul(letter)
                .ok_or(ScoreError::Overflow)?;
            sum = sum.checked_add(value).ok_or(ScoreError::Overflow)?;
            multiplier = multiplier.checked_mul(word).ok_or(ScoreError::Overflow)?;
        }
        let score = sum.checked_mul(multiplier).ok_or(ScoreError::Overflow)?;
        total = total.checked_add(score).ok_or(ScoreError::Overflow)?;
        word_scores.push(score);
    }
    let bingo_bonus = if validated.placements().len() == usize::from(rules.rack_size) {
        u32::from(rules.bingo_bonus)
    } else {
        0
    };
    total = total.checked_add(bingo_bonus).ok_or(ScoreError::Overflow)?;
    Ok(MoveScore {
        word_scores,
        bingo_bonus,
        total,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoredCommitError {
    NotActive,
    Placement(PlacementError),
    Score(ScoreError),
    State(StateError),
    CumulativeOverflow,
}
impl fmt::Display for ScoredCommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ScoredCommitError {}
/// Storage-level scored commit. Dictionary, sequence, draw and turn transaction follow in #19/#24.
/// Validates before transferring tiles; errors leave the board and cumulative score unchanged.
pub fn commit_placement(
    state: &mut GameState,
    player: PlayerId,
    placements: &[Placement],
) -> Result<MoveScore, ScoredCommitError> {
    if !matches!(state.phase(),Phase::Playing {active_player,..} if *active_player==player) {
        return Err(ScoredCommitError::NotActive);
    }
    let validated =
        placement::validate(state, player, placements).map_err(ScoredCommitError::Placement)?;
    let score = calculate(&validated).map_err(ScoredCommitError::Score)?;
    let delta = i32::try_from(score.total).map_err(|_| ScoredCommitError::CumulativeOverflow)?;
    let current = state
        .player_score(player)
        .ok_or(ScoredCommitError::State(StateError::UnknownPlayer))?;
    let updated = current
        .checked_add(delta)
        .ok_or(ScoredCommitError::CumulativeOverflow)?;
    state
        .commit_tiles_with_score(player, placements, updated)
        .map_err(ScoredCommitError::State)?;
    Ok(score)
}
