//! Scrabble payloads carried inside game-server's authenticated transport envelope.
use crate::identity::{
    ContractError, Coordinate, GameId, PlayerId, ProtocolVersion, TileId, TurnId,
};
use crate::ruleset::{ContentIdentity, TileFace};
use serde::{Deserialize, Serialize};
use std::fmt;

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
    /// Runtime presentation evidence, excluded from canonical game state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connected: Option<bool>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardTile {
    pub coordinate: Coordinate,
    pub letter: char,
    pub is_blank: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewTile {
    pub coordinate: Coordinate,
    pub letter: char,
    pub is_blank: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicPreview {
    pub player_id: PlayerId,
    pub turn: TurnId,
    pub tiles: Vec<PreviewTile>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicSnapshot {
    pub version: ProtocolVersion,
    pub game_id: GameId,
    pub revision: TurnId,
    pub ruleset: ContentIdentity,
    pub dictionary: ContentIdentity,
    pub phase: Phase,
    pub board: Vec<BoardTile>,
    pub players: Vec<PublicPlayer>,
    pub remaining_tiles: u16,
    /// Ephemeral projection. Never part of canonical replay or score calculation.
    pub preview: Option<PublicPreview>,
    /// Present only for matches with a shared board outside the playing roster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<PublicHost>,
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
}
