//! Scrabble payloads carried inside game-server's authenticated transport envelope.
use crate::identity::{
    ContractError, Coordinate, GameId, PlayerId, ProtocolVersion, TileId, TurnId,
};
use crate::ruleset::{ContentIdentity, TileFace};
use serde::{Deserialize, Serialize};

pub const MAX_COMMAND_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub tile_id: TileId,
    pub coordinate: Coordinate,
    pub blank_as: Option<char>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Start {},
    SetName { display_name: String },
    Preview { placements: Vec<Placement> },
    Commit { placements: Vec<Placement> },
    Pass {},
    Exchange { tile_ids: Vec<TileId> },
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
        let envelope: Self =
            serde_json::from_slice(bytes).map_err(|error| ContractError(error.to_string()))?;
        let count = match &envelope.command {
            Command::Preview { placements } | Command::Commit { placements } => placements.len(),
            Command::Exchange { tile_ids } => tile_ids.len(),
            Command::Start {} | Command::Pass {} | Command::SetName { .. } => 0,
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
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackTile {
    pub id: TileId,
    pub face: TileFace,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateRack {
    pub player_id: PlayerId,
    pub tiles: Vec<RackTile>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSnapshot {
    pub public: PublicSnapshot,
    pub own_rack: PrivateRack,
}
