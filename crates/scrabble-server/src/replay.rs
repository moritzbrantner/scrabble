//! Offline verification consumes native authority; it never maintains a gameplay log.
use crate::{ErrorResult, factory::MatchFactory};
use game_server::{MatchId, RecoveryImage, verify_replay};
use scrabble_game::{
    identity::GameId,
    protocol::{Phase, PublicTurn},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct ReplayGame {
    pub game_id: GameId,
    pub phase: Phase,
    pub history: Vec<PublicTurn>,
}
#[derive(Serialize)]
pub struct ReplayReport {
    pub records_verified: usize,
    pub checkpoints_verified: usize,
    pub final_tick: u64,
    pub canonical_hash: String,
    pub game: ReplayGame,
}

/// Factory inputs must match the persisted seed, rules and dictionary used by the host.
pub fn inspect_replay(
    factory: &MatchFactory,
    id: &MatchId,
    image: &RecoveryImage,
) -> ErrorResult<ReplayReport> {
    let verified = verify_replay(factory.create(id)?, &image.replay)?;
    // Select public fields only from the successfully verified canonical result.
    #[derive(Deserialize)]
    struct CanonicalSummary {
        game: ReplayGame,
    }
    let summary: CanonicalSummary = serde_json::from_slice(&verified.final_snapshot.payload)?;
    Ok(ReplayReport {
        records_verified: verified.records_verified,
        checkpoints_verified: verified.checkpoints_verified,
        final_tick: verified.final_snapshot.tick,
        canonical_hash: format!("{:016x}", verified.final_snapshot.state_hash),
        game: summary.game,
    })
}
