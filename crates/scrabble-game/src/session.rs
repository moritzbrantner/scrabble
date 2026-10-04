//! One application boundary for transport-sequenced commands; no transport or UI dependency.
use crate::identity::{GameId, PlayerId, TurnId};
use crate::protocol::{
    Command, CommandEnvelope, Phase, PlayerSnapshot, PublicPreview, PublicSnapshot,
};
use crate::state::{GameState, StateError};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandError {
    WrongGame,
    WrongIdentity,
    WrongSequence,
    StaleSequence,
    StaleTurn,
    NotActive,
    NotHost,
    UnsupportedCommand,
    TooManyTiles,
    State(StateError),
}
impl From<StateError> for CommandError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CommandError {}

pub struct GameSession {
    state: GameState,
    last_sequences: BTreeMap<PlayerId, u32>,
    preview: Option<PublicPreview>,
}
impl GameSession {
    pub fn new(state: GameState) -> Self {
        Self {
            state,
            last_sequences: BTreeMap::new(),
            preview: None,
        }
    }
    pub fn state(&self) -> &GameState {
        &self.state
    }
    pub fn add_player(&mut self, player: PlayerId, name: String) -> Result<(), StateError> {
        self.state.add_player(player, name)
    }
    pub fn remove_lobby_player(&mut self, player: PlayerId) -> Result<(), StateError> {
        self.state.remove_lobby_player(player)?;
        self.last_sequences.remove(&player);
        self.clear_preview_for(player);
        Ok(())
    }
    pub fn public_snapshot(&self) -> PublicSnapshot {
        let mut snapshot = self.state.public_snapshot();
        snapshot.preview = self.preview.clone();
        snapshot
    }
    pub fn player_snapshot(&self, player: PlayerId) -> Result<PlayerSnapshot, StateError> {
        let mut snapshot = self.state.player_snapshot(player)?;
        snapshot.public.preview = self.preview.clone();
        Ok(snapshot)
    }
    /// Transport supplies the authenticated player and sequence; payload claims must agree.
    /// Failed commands consume neither sequence nor turn and leave all state unchanged.
    pub fn apply(
        &mut self,
        player: PlayerId,
        sequence: u32,
        envelope: &CommandEnvelope,
    ) -> Result<(), CommandError> {
        if envelope.game_id != self.state.game_id() {
            return Err(CommandError::WrongGame);
        }
        if player != envelope.player_id {
            return Err(CommandError::WrongIdentity);
        }
        if sequence == 0 || sequence != envelope.sequence {
            return Err(CommandError::WrongSequence);
        }
        if sequence <= self.last_sequences.get(&player).copied().unwrap_or(0) {
            return Err(CommandError::StaleSequence);
        }
        if self.state.rack(player).is_none() {
            return Err(StateError::UnknownPlayer.into());
        }
        match self.state.phase() {
            Phase::Lobby {} => {
                if envelope.expected_turn != TurnId::new(0) {
                    return Err(CommandError::StaleTurn);
                }
                match &envelope.command {
                    Command::SetName { display_name } => {
                        self.state.set_display_name(player, display_name.clone())?
                    }
                    Command::Start {} => {
                        if self.state.host() != Some(player) {
                            return Err(CommandError::NotHost);
                        }
                        self.state.deal_initial_racks()?;
                    }
                    _ => return Err(StateError::WrongPhase.into()),
                }
            }
            Phase::Playing {
                active_player,
                turn,
            } => {
                if envelope.expected_turn != *turn {
                    return Err(CommandError::StaleTurn);
                }
                if player != *active_player {
                    return Err(CommandError::NotActive);
                }
                match &envelope.command {
                    Command::Preview { placements } => {
                        if placements.len() > usize::from(self.state.ruleset().rack_size) {
                            return Err(CommandError::TooManyTiles);
                        }
                        let tiles = self.state.preview_tiles(player, placements)?;
                        self.preview = if tiles.is_empty() {
                            None
                        } else {
                            Some(PublicPreview {
                                player_id: player,
                                turn: *turn,
                                tiles,
                            })
                        };
                    }
                    Command::Pass {} => {
                        self.state.advance_turn()?;
                        self.preview = None;
                    }
                    Command::Start {} | Command::SetName { .. } => {
                        return Err(StateError::WrongPhase.into());
                    }
                    // Commit requires structural/dictionary/scoring integration (#5, #6, #19, #24).
                    Command::Commit { .. } | Command::Exchange { .. } => {
                        return Err(CommandError::UnsupportedCommand);
                    }
                }
            }
            Phase::Finished { .. } => return Err(StateError::WrongPhase.into()),
        }
        self.last_sequences.insert(player, sequence);
        Ok(())
    }
    /// Connection replacement/loss clears presentation evidence without changing the game.
    pub fn clear_preview_for(&mut self, player: PlayerId) {
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.player_id == player)
        {
            self.preview = None;
        }
    }
    pub fn game_id(&self) -> GameId {
        self.state.game_id()
    }
}
