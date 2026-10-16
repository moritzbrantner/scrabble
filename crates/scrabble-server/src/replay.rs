//! Offline verification consumes native authority; it never maintains a gameplay log.
use crate::{ErrorResult, factory::MatchFactory};
use game_server::{MatchId, RecoveryImage, verify_replay};
use scrabble_game::{
    identity::{GameId, PlayerId},
    protocol::{Phase, PublicPlayer, PublicTurn},
    ruleset::Ruleset,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct ReplayGame {
    pub game_id: GameId,
    pub phase: Phase,
    pub history: Vec<PublicTurn>,
    pub players: Vec<PublicPlayer>,
}
#[derive(Serialize)]
pub struct ReplayReport {
    pub records_verified: usize,
    pub checkpoints_verified: usize,
    pub final_tick: u64,
    pub canonical_hash: String,
    pub configuration: Ruleset,
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
        game: CanonicalGame,
    }
    #[derive(Deserialize)]
    struct CanonicalGame {
        game_id: GameId,
        phase: Phase,
        history: Vec<PublicTurn>,
        ruleset: Ruleset,
        players: Vec<CanonicalPlayer>,
    }
    #[derive(Deserialize)]
    struct CanonicalPlayer {
        id: PlayerId,
        display_name: String,
        score: i32,
        // Retain only the count, never tile identities in the public report.
        rack: Vec<serde::de::IgnoredAny>,
        #[serde(default)]
        forfeited: bool,
    }
    let summary: CanonicalSummary = serde_json::from_slice(&verified.final_snapshot.payload)?;
    let players = summary
        .game
        .players
        .into_iter()
        .map(|player| {
            Ok(PublicPlayer {
                id: player.id,
                display_name: player.display_name,
                score: player.score,
                rack_count: u8::try_from(player.rack.len())?,
                forfeited: player.forfeited,
                connected: None,
            })
        })
        .collect::<ErrorResult<Vec<_>>>()?;
    Ok(ReplayReport {
        records_verified: verified.records_verified,
        checkpoints_verified: verified.checkpoints_verified,
        final_tick: verified.final_snapshot.tick,
        canonical_hash: format!("{:016x}", verified.final_snapshot.state_hash),
        configuration: summary.game.ruleset,
        game: ReplayGame {
            game_id: summary.game.game_id,
            phase: summary.game.phase,
            history: summary.game.history,
            players,
        },
    })
}
