//! Scrabble payloads carried inside game-server's authenticated transport envelope.
use crate::identity::{
    ContractError, Coordinate, GameId, PlayerId, ProtocolVersion, TileId, TurnId,
};
use crate::ruleset::{ContentIdentity, Ruleset, TileFace};
use serde::{Deserialize, Serialize};
use std::fmt;

pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

pub const MAX_COMMAND_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub tile_id: TileId,
    pub coordinate: Coordinate,
    pub blank_as: Option<char>,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Start {},
    ClaimBoard { request_id: String },
    SetName { display_name: String },
    Preview { placements: Vec<Placement> },
    Commit { placements: Vec<Placement> },
    Pass {},
    Exchange { tile_ids: Vec<TileId> },
}
impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, tiles) = match self {
            Self::Start {} => ("Start", None),
            Self::Pass {} => ("Pass", None),
            Self::ClaimBoard { .. } => ("ClaimBoard", None),
            Self::SetName { .. } => ("SetName", None),
            Self::Preview { placements } => ("Preview", Some(placements.len())),
            Self::Commit { placements } => ("Commit", Some(placements.len())),
            Self::Exchange { tile_ids } => ("Exchange", Some(tile_ids.len())),
        };
        let mut result = f.debug_struct(kind);
        if let Some(count) = tiles {
            result.field("tile_count", &count);
        }
        result.finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope {
    pub version: ProtocolVersion,
    pub game_id: GameId,
    /// A claim to compare against the transport-authenticated player, never authority by itself.
    pub player_id: PlayerId,
    /// Transport sequence must match; game-server owns connection-epoch fencing.
    pub sequence: u32,
    pub expected_turn: TurnId,
    pub command: Command,
}
impl CommandEnvelope {
    pub fn decode(bytes: &[u8]) -> Result<Self, ContractError> {
        if bytes.len() > MAX_COMMAND_BYTES {
            return Err(ContractError("command exceeds byte limit".into()));
        }
        let envelope: Self = serde_json::from_slice(bytes)
            .map_err(|_| ContractError("invalid command encoding".into()))?;
        let count = match &envelope.command {
            Command::Preview { placements } | Command::Commit { placements } => placements.len(),
            Command::Exchange { tile_ids } => tile_ids.len(),
            Command::Start {}
            | Command::Pass {}
            | Command::SetName { .. }
            | Command::ClaimBoard { .. } => 0,
        };
        if count > 15 {
            return Err(ContractError("command exceeds tile limit".into()));
        }
        Ok(envelope)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Phase {
    Lobby {},
    Playing {
        active_player: PlayerId,
        turn: TurnId,
    },
    Finished {
        winners: Vec<PlayerId>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicPlayer {
    pub id: PlayerId,
    pub display_name: String,
    pub score: i32,
    pub rack_count: u8,
    /// Eliminated by the immutable forfeit policy; omitted for existing matches.
    #[serde(default, skip_serializing_if = "is_false")]
    pub forfeited: bool,
    /// Runtime presentation evidence, excluded from canonical game state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connected: Option<bool>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardTile {
    pub coordinate: Coordinate,
    /// The whole spelled face (`"CH"` for a multi-letter tile); one character otherwise.
    pub letter: String,
    pub is_blank: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewTile {
    pub coordinate: Coordinate,
    /// The whole spelled face, as in [`BoardTile::letter`].
    pub letter: String,
    pub is_blank: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// The authenticated player's own tentative placement; only ever in that player's projection.
pub struct PrivatePreview {
    pub player_id: PlayerId,
    pub turn: TurnId,
    pub tiles: Vec<PreviewTile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<PlacementEvaluation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysedWord {
    pub text: String,
    pub direction: crate::placement::Direction,
    pub score: u32,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveAnalysis {
    pub words: Vec<AnalysedWord>,
    pub length_metric: crate::ruleset::WordLengthMetric,
    pub bingo_bonus: u32,
    pub total: u32,
}
/// Scores reference `TurnAction::Commit.words` by order; do not duplicate long spellings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryAnalysis {
    pub word_scores: Vec<(crate::placement::Direction, u32)>,
    pub length_metric: crate::ruleset::WordLengthMetric,
    pub bingo_bonus: u32,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlacementEvaluation {
    Valid {
        analysis: MoveAnalysis,
    },
    Invalid {
        problem: EvaluationProblem,
        words: Vec<String>,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationProblem {
    Center,
    NotLine,
    Gap,
    Disconnected,
    NoWord,
    Unavailable,
    InvalidWords,
    Dictionary,
    ScoreOverflow,
}
/// Bounded projection; the complete accepted commands remain in native replay evidence.
pub const MAX_HISTORY_TURNS: usize = 24;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TurnAction {
    Forfeit {},
    Commit {
        words: Vec<String>,
        move_score: u32,
        blank_count: u8,
    },
    Pass {},
    Exchange {
        tile_count: u8,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreChange {
    pub player_id: PlayerId,
    /// Includes final rack deductions/transfers; i64 accommodates any i32 score difference.
    pub delta: i64,
    pub score: i32,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicTurn {
    pub turn: TurnId,
    pub player_id: PlayerId,
    pub action: TurnAction,
    pub scores: Vec<ScoreChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<HistoryAnalysis>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicSnapshot {
    pub version: ProtocolVersion,
    pub game_id: GameId,
    pub revision: TurnId,
    pub ruleset: ContentIdentity,
    /// Immutable public geometry, alphabet and scoring data; never contains dictionary words.
    pub configuration: Ruleset,
    pub dictionary: ContentIdentity,
    pub phase: Phase,
    pub board: Vec<BoardTile>,
    pub players: Vec<PublicPlayer>,
    pub remaining_tiles: u16,
    pub history: Vec<PublicTurn>,
    /// Present only for matches with a shared board outside the playing roster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<PublicHost>,
    /// Timed matches only: balances and the active expiry instant, no private data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<PublicClock>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicClock {
    /// Seat order.
    pub players: Vec<PlayerClock>,
    pub turn_started_at_ms: Option<u64>,
    pub expires_at_ms: Option<u64>,
    /// Public projection reconstructed by authoritative timeout events; never a browser input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_timeout: Option<ClockTimeout>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockTimeout {
    pub player_id: PlayerId,
    pub turn: TurnId,
    pub consequence: crate::ruleset::ExpiryConsequence,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerClock {
    pub player_id: PlayerId,
    pub remaining_ms: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicHost {
    pub id: Option<PlayerId>,
}
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackTile {
    pub id: TileId,
    pub face: TileFace,
}
impl fmt::Debug for RackTile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RackTile").finish_non_exhaustive()
    }
}
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateRack {
    pub player_id: PlayerId,
    pub tiles: Vec<RackTile>,
}
impl fmt::Debug for PrivateRack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateRack")
            .field("player_id", &self.player_id)
            .field("tile_count", &self.tiles.len())
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSnapshot {
    pub public: PublicSnapshot,
    pub own_rack: PrivateRack,
    /// Ephemeral echo of this player's own proposed placement. Never public, canonical or
    /// part of score calculation; other players and the shared board never receive it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_preview: Option<PrivatePreview>,
}
