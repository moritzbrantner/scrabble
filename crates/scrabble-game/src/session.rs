//! One application boundary for transport-sequenced commands; no transport or UI dependency.
use crate::dictionary::{Dictionary, authored_fixture};
use crate::identity::{GameId, PlayerId, TurnId};
use crate::protocol::{
    Command, CommandEnvelope, Phase, PlayerSnapshot, PublicPreview, PublicSnapshot, TurnAction,
};
use crate::ruleset::ExpiryConsequence;
use crate::state::{ClockError, GameState, StateError};
use std::{collections::BTreeMap, fmt, sync::Arc};

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
    Commit(crate::commit::CommitError),
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
    dictionary: Arc<dyn Dictionary>,
    last_sequences: BTreeMap<PlayerId, u32>,
    preview: Option<PublicPreview>,
}
impl GameSession {
    pub fn new(state: GameState) -> Self {
        Self::with_dictionary(state, authored_fixture())
    }
    pub fn with_dictionary(state: GameState, dictionary: Arc<dyn Dictionary>) -> Self {
        Self {
            state,
            dictionary,
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
        self.forget_actor(player);
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
        self.apply_inner(player, sequence, envelope, None)
    }
    /// Like `apply`, at an authoritative instant. Untimed matches ignore the instant.
    pub fn apply_at(
        &mut self,
        player: PlayerId,
        sequence: u32,
        envelope: &CommandEnvelope,
        now_ms: u64,
    ) -> Result<(), CommandError> {
        self.apply_inner(player, sequence, envelope, Some(now_ms))
    }
    /// Server-issued expiry transition for the active turn. Consumes no sequence.
    pub fn timeout_at(&mut self, expected_turn: TurnId, now_ms: u64) -> Result<(), CommandError> {
        let Some(control) = self.state.ruleset().time_control else {
            return Err(StateError::Clock(ClockError::Untimed).into());
        };
        let Phase::Playing { turn, .. } = self.state.phase() else {
            return Err(StateError::WrongPhase.into());
        };
        if expected_turn != *turn {
            return Err(CommandError::StaleTurn);
        }
        match self.state.check_clock(now_ms) {
            Err(StateError::Clock(ClockError::Expired)) => {}
            Err(error) => return Err(error.into()),
            Ok(()) => return Err(StateError::Clock(ClockError::NotExpired).into()),
        }
        if control.on_expiry == ExpiryConsequence::Forfeit {
            return Err(StateError::Clock(ClockError::ForfeitUnavailable).into());
        }
        let mut candidate = self.state.clone();
        candidate.settle_clock_turn(now_ms, true)?;
        candidate.finish_turn(&self.state, 0, TurnAction::Pass {})?;
        candidate.begin_clock_turn(now_ms)?;
        self.state = candidate;
        self.preview = None;
        Ok(())
    }
    fn apply_inner(
        &mut self,
        player: PlayerId,
        sequence: u32,
        envelope: &CommandEnvelope,
        now_ms: Option<u64>,
    ) -> Result<(), CommandError> {
        let timed_now = |state: &GameState| -> Result<Option<u64>, CommandError> {
            if !state.is_timed() {
                return Ok(None);
            }
            now_ms
                .map(Some)
                .ok_or(StateError::Clock(ClockError::InstantRequired).into())
        };
        self.validate_actor(player, sequence, envelope)?;
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
                        match timed_now(&self.state)? {
                            Some(now) => self.state.deal_initial_racks_at(now)?,
                            None => self.state.deal_initial_racks()?,
                        }
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
                let clock_now = match &envelope.command {
                    Command::Start {} | Command::SetName { .. } | Command::ClaimBoard { .. } => {
                        None
                    }
                    _ => timed_now(&self.state)?,
                };
                if let Some(now) = clock_now {
                    self.state.check_clock(now)?;
                }
                // Settles the ending turn's clock and starts the next, on the candidate.
                let finish = |candidate: &mut GameState,
                              previous: &GameState,
                              action: TurnAction|
                 -> Result<(), StateError> {
                    if let Some(now) = clock_now {
                        candidate.settle_clock_turn(now, false)?;
                    }
                    candidate.finish_turn(previous, 0, action)?;
                    if let Some(now) = clock_now {
                        candidate.begin_clock_turn(now)?;
                    }
                    Ok(())
                };
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
                                evaluation: (self.state.ruleset().board_shape
                                    == crate::ruleset::BoardShape::Hexagon)
                                    .then(|| {
                                        crate::analysis::evaluate(
                                            &self.state,
                                            player,
                                            placements,
                                            self.dictionary.as_ref(),
                                        )
                                    }),
                            })
                        };
                    }
                    Command::Pass {} => {
                        let mut candidate = self.state.clone();
                        finish(&mut candidate, &self.state, TurnAction::Pass {})?;
                        self.state = candidate;
                        self.preview = None;
                    }
                    Command::Start {} | Command::SetName { .. } | Command::ClaimBoard { .. } => {
                        return Err(StateError::WrongPhase.into());
                    }
                    Command::Commit { placements } => {
                        crate::commit::apply_with_dictionary_at(
                            &mut self.state,
                            player,
                            placements,
                            self.dictionary.as_ref(),
                            clock_now,
                        )
                        .map_err(CommandError::Commit)?;
                        self.preview = None;
                    }
                    Command::Exchange { tile_ids } => {
                        let mut candidate = self.state.clone();
                        candidate.exchange_tiles(player, tile_ids)?;
                        finish(
                            &mut candidate,
                            &self.state,
                            TurnAction::Exchange {
                                tile_count: tile_ids.len() as u8,
                            },
                        )?;
                        self.state = candidate;
                        self.preview = None;
                    }
                }
            }
            Phase::Finished { .. } => return Err(StateError::WrongPhase.into()),
        }
        self.last_sequences.insert(player, sequence);
        Ok(())
    }
    fn validate_actor(
        &self,
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
        Ok(())
    }

    /// Separate-board matches admit transport actors before assigning playing seats.
    /// The simulation has already ruled out the board identity before calling this.
    pub fn join_named_player(
        &mut self,
        player: PlayerId,
        sequence: u32,
        envelope: &CommandEnvelope,
    ) -> Result<(), CommandError> {
        self.validate_lobby_actor(player, sequence, envelope)?;
        let Command::SetName { display_name } = &envelope.command else {
            return Err(CommandError::UnsupportedCommand);
        };
        if self.state.rack(player).is_some() {
            self.state.set_display_name(player, display_name.clone())?;
        } else {
            self.state.add_player(player, display_name.clone())?;
        }
        self.last_sequences.insert(player, sequence);
        Ok(())
    }

    /// The adapter must verify board ownership before this trusted operation.
    pub fn start_by_authorized_board(
        &mut self,
        player: PlayerId,
        sequence: u32,
        envelope: &CommandEnvelope,
    ) -> Result<(), CommandError> {
        self.validate_lobby_actor(player, sequence, envelope)?;
        if !matches!(envelope.command, Command::Start {}) {
            return Err(CommandError::UnsupportedCommand);
        }
        self.state.deal_initial_racks()?;
        self.last_sequences.insert(player, sequence);
        Ok(())
    }

    /// `start_by_authorized_board` at an authoritative instant; starts a timed clock.
    pub fn start_by_authorized_board_at(
        &mut self,
        player: PlayerId,
        sequence: u32,
        envelope: &CommandEnvelope,
        now_ms: u64,
    ) -> Result<(), CommandError> {
        self.validate_lobby_actor(player, sequence, envelope)?;
        if !matches!(envelope.command, Command::Start {}) {
            return Err(CommandError::UnsupportedCommand);
        }
        self.state.deal_initial_racks_at(now_ms)?;
        self.last_sequences.insert(player, sequence);
        Ok(())
    }

    /// Record an authenticated board claim only after its private proof is verified.
    pub fn accept_board_claim(
        &mut self,
        player: PlayerId,
        sequence: u32,
        envelope: &CommandEnvelope,
    ) -> Result<(), CommandError> {
        self.validate_lobby_actor(player, sequence, envelope)?;
        if !matches!(envelope.command, Command::ClaimBoard { .. }) {
            return Err(CommandError::UnsupportedCommand);
        }
        self.last_sequences.insert(player, sequence);
        Ok(())
    }

    fn validate_lobby_actor(
        &self,
        player: PlayerId,
        sequence: u32,
        envelope: &CommandEnvelope,
    ) -> Result<(), CommandError> {
        self.validate_actor(player, sequence, envelope)?;
        if !matches!(self.state.phase(), Phase::Lobby {}) {
            return Err(StateError::WrongPhase.into());
        }
        if envelope.expected_turn != TurnId::new(0) {
            return Err(CommandError::StaleTurn);
        }
        Ok(())
    }

    /// Transport expiry ends command-sequence retention, including actors without seats.
    pub fn forget_actor(&mut self, player: PlayerId) {
        self.last_sequences.remove(&player);
        self.clear_preview_for(player);
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
