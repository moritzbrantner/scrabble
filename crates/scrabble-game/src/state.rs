//! Canonical tile storage. Placement transfer enforces storage invariants, not move legality.
//! Callers must validate turn, words and scoring before submitting a gameplay transaction.
use crate::identity::{
    ContractError, Coordinate, GameId, PlayerId, ProtocolVersion, TileId, TurnId,
};
use crate::protocol::{
    BoardTile, Phase, Placement, PlayerSnapshot, PrivateRack, PublicPlayer, PublicSnapshot,
    RackTile,
};
use crate::ruleset::{Ruleset, TileFace};
use rand::{SeedableRng, seq::SliceRandom};
use rand_chacha::ChaCha8Rng;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateError {
    InvalidRuleset(ContractError),
    DuplicatePlayer,
    PlayerLimit,
    InvalidName,
    WrongPhase,
    NotEnoughPlayers,
    UnknownPlayer,
    DuplicateTile,
    DuplicateCoordinate,
    TileNotOwned,
    OccupiedSquare,
    OutsideBoard,
    InvalidBlankAssignment,
    InvariantViolation,
    CounterExhausted,
}
impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRuleset(error) => return write!(f, "invalid ruleset: {error}"),
            Self::DuplicatePlayer => "player already admitted",
            Self::PlayerLimit => "player limit reached",
            Self::InvalidName => "display name must contain 1 to 32 characters",
            Self::WrongPhase => "operation not available in this phase",
            Self::NotEnoughPlayers => "not enough players to deal",
            Self::UnknownPlayer => "unknown player",
            Self::DuplicateTile => "duplicate tile identity",
            Self::DuplicateCoordinate => "duplicate board coordinate",
            Self::TileNotOwned => "tile not in player's rack",
            Self::OccupiedSquare => "board square already occupied",
            Self::OutsideBoard => "coordinate outside configured board",
            Self::InvalidBlankAssignment => "invalid blank assignment",
            Self::CounterExhausted => "authoritative counter exhausted",
            Self::InvariantViolation => "canonical tile conservation failed",
        };
        f.write_str(message)
    }
}
impl std::error::Error for StateError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Tile {
    id: TileId,
    face: TileFace,
    value: u16,
}
impl Tile {
    pub const fn id(&self) -> TileId {
        self.id
    }
    pub const fn face(&self) -> TileFace {
        self.face
    }
    pub const fn value(&self) -> u16 {
        self.value
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommittedTile {
    tile: Tile,
    letter: char,
}
impl CommittedTile {
    pub fn tile(&self) -> &Tile {
        &self.tile
    }
    pub const fn letter(&self) -> char {
        self.letter
    }
    pub const fn is_blank(&self) -> bool {
        matches!(self.tile.face, TileFace::Blank {})
    }
}
#[derive(Clone, Serialize)]
struct Player {
    id: PlayerId,
    display_name: String,
    score: i32,
    rack: Vec<Tile>,
}

/// Owned canonical state; no Deserialize or public mutable collections can bypass invariants.
/// Deliberately has no Debug/Serialize implementation exposing private racks or bag order.
#[derive(Clone)]
pub struct GameState {
    game_id: GameId,
    ruleset: Ruleset,
    phase: Phase,
    revision: TurnId,
    players: Vec<Player>,
    bag: Vec<Tile>,
    board: Vec<Option<CommittedTile>>,
}

fn make_tiles(rules: &Ruleset) -> Vec<Tile> {
    let mut result = Vec::new();
    for definition in &rules.tiles {
        for _ in 0..definition.count {
            result.push(Tile {
                id: TileId::new(result.len() as u64),
                face: definition.face,
                value: definition.value,
            });
        }
    }
    result
}
impl GameState {
    pub fn new(game_id: GameId, ruleset: Ruleset, seed: [u8; 32]) -> Result<Self, StateError> {
        ruleset.validate().map_err(StateError::InvalidRuleset)?;
        let mut bag = make_tiles(&ruleset);
        // Explicit ChaCha variant, full seed and locked shuffle implementation form replay inputs.
        bag.shuffle(&mut ChaCha8Rng::from_seed(seed));
        let board = vec![None; usize::from(ruleset.board_size).pow(2)];
        Ok(Self {
            game_id,
            ruleset,
            phase: Phase::Lobby {},
            revision: TurnId::new(0),
            players: vec![],
            bag,
            board,
        })
    }
    pub fn ruleset(&self) -> &Ruleset {
        &self.ruleset
    }
    pub fn phase(&self) -> &Phase {
        &self.phase
    }
    pub fn remaining_tiles(&self) -> usize {
        self.bag.len()
    }
    pub fn rack(&self, player: PlayerId) -> Option<&[Tile]> {
        self.players
            .iter()
            .find(|entry| entry.id == player)
            .map(|entry| entry.rack.as_slice())
    }
    fn board_index(&self, coordinate: Coordinate) -> Result<usize, StateError> {
        if coordinate.row() >= self.ruleset.board_size
            || coordinate.column() >= self.ruleset.board_size
        {
            return Err(StateError::OutsideBoard);
        }
        Ok(
            usize::from(coordinate.row()) * usize::from(self.ruleset.board_size)
                + usize::from(coordinate.column()),
        )
    }
    pub(crate) fn board_is_empty(&self) -> bool {
        self.board.iter().all(Option::is_none)
    }
    pub fn board_tile(&self, coordinate: Coordinate) -> Option<&CommittedTile> {
        self.board_index(coordinate)
            .ok()
            .and_then(|index| self.board[index].as_ref())
    }
    fn next_revision(&self) -> Result<TurnId, StateError> {
        self.revision
            .get()
            .checked_add(1)
            .map(TurnId::new)
            .ok_or(StateError::InvariantViolation)
    }
    pub fn add_player(&mut self, id: PlayerId, display_name: String) -> Result<(), StateError> {
        if !matches!(self.phase, Phase::Lobby {}) {
            return Err(StateError::WrongPhase);
        }
        if self.players.iter().any(|player| player.id == id) {
            return Err(StateError::DuplicatePlayer);
        }
        if self.players.len() >= usize::from(self.ruleset.maximum_players) {
            return Err(StateError::PlayerLimit);
        }
        let name = display_name.trim();
        if name.is_empty() || name.chars().count() > 32 || name.chars().any(char::is_control) {
            return Err(StateError::InvalidName);
        }
        let revision = self.next_revision()?;
        self.players.push(Player {
            id,
            display_name: name.to_owned(),
            score: 0,
            rack: vec![],
        });
        self.revision = revision;
        Ok(())
    }
    pub fn deal_initial_racks(&mut self) -> Result<(), StateError> {
        if !matches!(self.phase, Phase::Lobby {}) {
            return Err(StateError::WrongPhase);
        }
        if self.players.len() < usize::from(self.ruleset.minimum_players) {
            return Err(StateError::NotEnoughPlayers);
        }
        let revision = self.next_revision()?;
        // Deal round-robin in stable seat order. Every rack starts empty and ruleset validates capacity.
        for _ in 0..self.ruleset.rack_size {
            for player in &mut self.players {
                if let Some(tile) = self.bag.pop() {
                    player.rack.push(tile);
                }
            }
        }
        self.phase = Phase::Playing {
            active_player: self.players[0].id,
            turn: TurnId::new(0),
        };
        self.revision = revision;
        Ok(())
    }
    pub fn refill_rack(&mut self, player: PlayerId) -> Result<usize, StateError> {
        if !matches!(self.phase, Phase::Playing { .. }) {
            return Err(StateError::WrongPhase);
        }
        let index = self
            .players
            .iter()
            .position(|entry| entry.id == player)
            .ok_or(StateError::UnknownPlayer)?;
        let count = (usize::from(self.ruleset.rack_size) - self.players[index].rack.len())
            .min(self.bag.len());
        if count == 0 {
            return Ok(0);
        }
        let revision = self.next_revision()?;
        let split = self.bag.len() - count;
        self.players[index]
            .rack
            .extend(self.bag.drain(split..).rev());
        self.revision = revision;
        Ok(count)
    }
    /// Storage-only atomic transfer. Does not establish turn authority, structural legality or score.
    pub fn place_tiles(
        &mut self,
        player: PlayerId,
        placements: &[Placement],
    ) -> Result<(), StateError> {
        if !matches!(self.phase, Phase::Playing { .. }) {
            return Err(StateError::WrongPhase);
        }
        let player_index = self
            .players
            .iter()
            .position(|entry| entry.id == player)
            .ok_or(StateError::UnknownPlayer)?;
        let transfers = self.validated_transfers(player, placements)?;
        if placements.is_empty() {
            return Ok(());
        }
        let revision = self.next_revision()?;
        // All fallible validation finishes above. Retain moves actual tiles instead of duplicating them.
        let mut remaining =
            Vec::with_capacity(self.players[player_index].rack.len() - transfers.len());
        for tile in self.players[player_index].rack.drain(..) {
            if let Some((_, index, letter)) = transfers.iter().find(|(id, _, _)| *id == tile.id) {
                self.board[*index] = Some(CommittedTile {
                    tile,
                    letter: *letter,
                });
            } else {
                remaining.push(tile);
            }
        }
        self.players[player_index].rack = remaining;
        self.revision = revision;
        Ok(())
    }
    fn validated_transfers(
        &self,
        player: PlayerId,
        placements: &[Placement],
    ) -> Result<Vec<(TileId, usize, char)>, StateError> {
        let player_index = self
            .players
            .iter()
            .position(|entry| entry.id == player)
            .ok_or(StateError::UnknownPlayer)?;
        let mut ids = BTreeSet::new();
        let mut coordinates = BTreeSet::new();
        let mut transfers = Vec::with_capacity(placements.len());
        for placement in placements {
            if !ids.insert(placement.tile_id) {
                return Err(StateError::DuplicateTile);
            }
            let index = self.board_index(placement.coordinate)?;
            if !coordinates.insert(index) {
                return Err(StateError::DuplicateCoordinate);
            }
            if self.board[index].is_some() {
                return Err(StateError::OccupiedSquare);
            }
            let tile = self.players[player_index]
                .rack
                .iter()
                .find(|tile| tile.id == placement.tile_id)
                .ok_or(StateError::TileNotOwned)?;
            let letter = match (tile.face, placement.blank_as) {
                (TileFace::Letter { letter }, None) => letter,
                (TileFace::Blank {}, Some(letter))
                    if self
                        .ruleset
                        .tiles
                        .iter()
                        .any(|definition| definition.face == TileFace::Letter { letter }) =>
                {
                    letter
                }
                _ => return Err(StateError::InvalidBlankAssignment),
            };
            transfers.push((tile.id, index, letter));
        }
        Ok(transfers)
    }
    pub(crate) fn preview_tiles(
        &self,
        player: PlayerId,
        placements: &[Placement],
    ) -> Result<Vec<crate::protocol::PreviewTile>, StateError> {
        let transfers = self.validated_transfers(player, placements)?;
        Ok(placements
            .iter()
            .zip(transfers)
            .map(|(placement, (_, _, letter))| crate::protocol::PreviewTile {
                coordinate: placement.coordinate,
                letter,
                is_blank: placement.blank_as.is_some(),
            })
            .collect())
    }
    pub(crate) fn game_id(&self) -> GameId {
        self.game_id
    }
    pub(crate) fn host(&self) -> Option<PlayerId> {
        self.players.first().map(|player| player.id)
    }
    pub(crate) fn advance_turn(&mut self) -> Result<(), StateError> {
        let Phase::Playing {
            active_player,
            turn,
        } = self.phase
        else {
            return Err(StateError::WrongPhase);
        };
        let index = self
            .players
            .iter()
            .position(|player| player.id == active_player)
            .ok_or(StateError::UnknownPlayer)?;
        let next_turn = turn
            .get()
            .checked_add(1)
            .ok_or(StateError::CounterExhausted)?;
        let revision = self.next_revision()?;
        self.phase = Phase::Playing {
            active_player: self.players[(index + 1) % self.players.len()].id,
            turn: TurnId::new(next_turn),
        };
        self.revision = revision;
        Ok(())
    }
    /// Private replay evidence. Never send these bytes through a browser snapshot connection.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        #[derive(Serialize)]
        struct Evidence<'a> {
            version: u16,
            game_id: GameId,
            ruleset: &'a Ruleset,
            phase: &'a Phase,
            revision: TurnId,
            players: &'a [Player],
            bag: &'a [Tile],
            board: &'a [Option<CommittedTile>],
        }
        serde_json::to_vec(&Evidence {
            version: 1,
            game_id: self.game_id,
            ruleset: &self.ruleset,
            phase: &self.phase,
            revision: self.revision,
            players: &self.players,
            bag: &self.bag,
            board: &self.board,
        })
    }
    pub fn public_snapshot(&self) -> PublicSnapshot {
        let size = usize::from(self.ruleset.board_size);
        let board = self
            .board
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                entry.as_ref().map(|tile| BoardTile {
                    // Constructor validated odd dimensions in 1..=15; row/column are provably in bounds.
                    coordinate: Coordinate::new((index / size) as u8, (index % size) as u8)
                        .expect("validated board dimensions"),
                    letter: tile.letter,
                    is_blank: tile.is_blank(),
                })
            })
            .collect();
        PublicSnapshot {
            version: ProtocolVersion,
            game_id: self.game_id,
            revision: self.revision,
            ruleset: self.ruleset.identity.clone(),
            dictionary: self.ruleset.dictionary.clone(),
            phase: self.phase.clone(),
            board,
            players: self
                .players
                .iter()
                .map(|player| PublicPlayer {
                    id: player.id,
                    display_name: player.display_name.clone(),
                    score: player.score,
                    rack_count: player.rack.len() as u8,
                })
                .collect(),
            remaining_tiles: self.bag.len() as u16,
            preview: None,
        }
    }
    /// Trusted in-process projection; the transport adapter must select player using authenticated identity.
    pub fn player_snapshot(&self, player: PlayerId) -> Result<PlayerSnapshot, StateError> {
        let rack = self.rack(player).ok_or(StateError::UnknownPlayer)?;
        Ok(PlayerSnapshot {
            public: self.public_snapshot(),
            own_rack: PrivateRack {
                player_id: player,
                tiles: rack
                    .iter()
                    .map(|tile| RackTile {
                        id: tile.id,
                        face: tile.face,
                    })
                    .collect(),
            },
        })
    }
    pub fn verify_tile_conservation(&self) -> Result<(), StateError> {
        let expected: BTreeMap<_, _> = make_tiles(&self.ruleset)
            .into_iter()
            .map(|tile| (tile.id, tile))
            .collect();
        let mut actual = BTreeMap::new();
        for tile in self
            .bag
            .iter()
            .chain(self.players.iter().flat_map(|player| player.rack.iter()))
            .chain(self.board.iter().flatten().map(|entry| &entry.tile))
        {
            if actual.insert(tile.id, tile.clone()).is_some() {
                return Err(StateError::InvariantViolation);
            }
        }
        if actual != expected {
            return Err(StateError::InvariantViolation);
        }
        Ok(())
    }
}

#[cfg(test)]
mod turn_counter_tests {
    use super::*;
    #[test]
    fn exhausted_turn_counter_does_not_mutate_state() {
        let mut state =
            GameState::new(GameId::new(1), crate::ruleset::english_fixture(), [0; 32]).unwrap();
        state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
        state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
        state.deal_initial_racks().unwrap();
        state.phase = Phase::Playing {
            active_player: PlayerId::new(1),
            turn: TurnId::new(u64::MAX),
        };
        let before = state.canonical_bytes().unwrap();
        assert_eq!(state.advance_turn(), Err(StateError::CounterExhausted));
        assert_eq!(state.canonical_bytes().unwrap(), before);
    }
}
