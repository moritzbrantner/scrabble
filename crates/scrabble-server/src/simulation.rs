use game_server::{GameSimulation, SimulationError, SimulationSnapshot, SnapshotScope};
use scrabble_game::{
    dictionary::{Dictionary, DictionaryError, authored_fixture},
    identity::{GameId, PlayerId},
    protocol::{Command, CommandEnvelope, Phase, PlayerSnapshot, PrivateRack, PublicHost},
    ruleset::Ruleset,
    session::{CommandError, GameSession},
    state::{GameState, StateError},
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::{collections::BTreeSet, sync::Arc};

pub const SCRABBLE_TICK_HZ: u16 = 20;
pub const PREVIEW_LIFETIME_TICKS: u64 = 2 * SCRABBLE_TICK_HZ as u64;

/// No Debug/Serialize: canonical state is private and requires an explicit snapshot call.
pub struct ScrabbleSimulation {
    session: GameSession,
    tick: u64,
    admitted: BTreeSet<u32>,
    last_admitted_player: Option<u32>,
    lifecycle: Option<MatchLifecycle>,
    board: Option<BoardLobby>,
    preview_deadline: Option<(PlayerId, u64)>,
}

struct BoardLobby {
    authority: crate::factory::BoardAuthority,
    owner: Option<u32>,
}

pub const MATCH_LIFETIME_SECONDS: u64 = 6 * 60 * 60;
pub const FINISHED_RETENTION_TICKS: u64 = 5 * 60 * SCRABBLE_TICK_HZ as u64;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchLifecycle {
    pub created_at: u64,
    pub finished_at_tick: Option<u64>,
}
impl MatchLifecycle {
    pub fn expires_at(&self) -> u64 {
        self.created_at.saturating_add(MATCH_LIFETIME_SECONDS)
    }
    pub fn can_retire(&self, now: u64, tick: u64) -> bool {
        let minimum_age = FINISHED_RETENTION_TICKS / u64::from(SCRABBLE_TICK_HZ);
        now >= self.expires_at()
            || (now.saturating_sub(self.created_at) >= minimum_age
                && self.finished_at_tick.is_some_and(|finished| {
                    tick.saturating_sub(finished) >= FINISHED_RETENTION_TICKS
                }))
    }
}

/// Only lifecycle fields are decoded; canonical data never reaches an HTTP response.
pub fn retirement_due(payload: &[u8], now: u64, tick: u64) -> Result<bool, serde_json::Error> {
    #[derive(Deserialize)]
    struct Projection {
        lifecycle: Option<MatchLifecycle>,
    }
    let projection: Projection = serde_json::from_slice(payload)?;
    Ok(projection
        .lifecycle
        .is_some_and(|lifecycle| lifecycle.can_retire(now, tick)))
}

fn command_error(error_value: CommandError, envelope: &CommandEnvelope) -> SimulationError {
    if let CommandError::Commit(scrabble_game::commit::CommitError::Dictionary(
        dictionary_error @ DictionaryError::InvalidWords { .. },
    )) = &error_value
    {
        #[derive(Serialize)]
        struct Feedback<'a> {
            version: u8,
            game_id: GameId,
            expected_turn: scrabble_game::identity::TurnId,
            error: &'a DictionaryError,
        }
        let payload = serde_json::to_vec(&Feedback {
            version: 1,
            game_id: envelope.game_id,
            expected_turn: envelope.expected_turn,
            error: dictionary_error,
        });
        return match payload {
            Ok(payload) => SimulationError::command_rejected(payload).unwrap_or_else(error),
            Err(failure) => error(failure),
        };
    }
    error(error_value)
}

fn error(error: impl std::fmt::Display) -> SimulationError {
    SimulationError::new(error.to_string())
}

impl ScrabbleSimulation {
    pub fn new(game_id: GameId, ruleset: Ruleset, seed: [u8; 32]) -> Result<Self, StateError> {
        Self::with_dictionary(game_id, ruleset, seed, authored_fixture())
    }

    pub fn with_dictionary(
        game_id: GameId,
        ruleset: Ruleset,
        seed: [u8; 32],
        dictionary: Arc<dyn Dictionary>,
    ) -> Result<Self, StateError> {
        Ok(Self {
            session: GameSession::with_dictionary(
                GameState::new(game_id, ruleset, seed)?,
                dictionary,
            ),
            tick: 0,
            admitted: BTreeSet::new(),
            last_admitted_player: None,
            lifecycle: None,
            board: None,
            preview_deadline: None,
        })
    }

    pub(crate) fn with_board_authority(
        mut self,
        authority: crate::factory::BoardAuthority,
    ) -> Self {
        self.board = Some(BoardLobby {
            authority,
            owner: None,
        });
        self
    }

    fn player_projection(&self, player: PlayerId) -> Result<PlayerSnapshot, SimulationError> {
        let mut snapshot = if self.board.is_some() && self.session.state().rack(player).is_none() {
            PlayerSnapshot {
                public: self.session.public_snapshot(),
                own_rack: PrivateRack {
                    player_id: player,
                    tiles: vec![],
                },
            }
        } else {
            self.session.player_snapshot(player).map_err(error)?
        };
        if let Some(board) = &self.board {
            snapshot.public.host = Some(PublicHost {
                id: board.owner.map(|id| PlayerId::new(u64::from(id))),
            });
        }
        Ok(snapshot)
    }

    pub fn with_lifecycle(mut self, created_at: u64) -> Self {
        self.lifecycle = Some(MatchLifecycle {
            created_at,
            finished_at_tick: None,
        });
        self
    }

    fn require_admitted(&self, player_id: u32) -> Result<PlayerId, SimulationError> {
        if !self.admitted.contains(&player_id) {
            return Err(SimulationError::new(
                "player is not admitted to this simulation",
            ));
        }
        Ok(PlayerId::new(u64::from(player_id)))
    }
}

impl GameSimulation for ScrabbleSimulation {
    fn connection_changed(&mut self, player_id: u32) {
        let player = PlayerId::new(u64::from(player_id));
        self.session.clear_preview_for(player);
        if self
            .preview_deadline
            .is_some_and(|(owner, _)| owner == player)
        {
            self.preview_deadline = None;
        }
    }
    fn tick_hz(&self) -> u16 {
        SCRABBLE_TICK_HZ
    }
    fn max_players(&self) -> usize {
        usize::from(self.session.state().ruleset().maximum_players)
            + usize::from(self.board.is_some())
    }
    fn current_tick(&self) -> u64 {
        self.tick
    }

    fn add_player(&mut self, player_id: u32) -> Result<(), SimulationError> {
        if self
            .last_admitted_player
            .is_some_and(|last| player_id <= last)
        {
            return Err(SimulationError::new(
                "player identities must increase and cannot be reassigned",
            ));
        }
        if self.board.is_some() {
            if !matches!(self.session.state().phase(), Phase::Lobby {}) {
                return Err(error(StateError::WrongPhase));
            }
            if self.admitted.len() >= self.max_players() {
                return Err(error(StateError::PlayerLimit));
            }
        } else {
            self.session
                .add_player(
                    PlayerId::new(u64::from(player_id)),
                    format!("Player {player_id}"),
                )
                .map_err(error)?;
        }
        self.admitted.insert(player_id);
        self.last_admitted_player = Some(player_id);
        Ok(())
    }

    fn remove_player(&mut self, player_id: u32) -> bool {
        // The runtime uses the fallible hook; this trait fallback reports refusal explicitly.
        self.try_remove_player(player_id).unwrap_or(false)
    }

    fn try_remove_player(&mut self, player_id: u32) -> Result<bool, SimulationError> {
        if !self.admitted.contains(&player_id) {
            return Ok(false);
        }
        let player = PlayerId::new(u64::from(player_id));
        if matches!(self.session.state().phase(), Phase::Lobby {}) {
            if self.session.state().rack(player).is_some() {
                self.session.remove_lobby_player(player).map_err(error)?;
            }
        } else {
            // Expiry never donates the rack or replaces a started game's seat.
            // An expired capability cannot reclaim a started seat.
            self.session.clear_preview_for(player);
        }
        self.session.forget_actor(player);
        self.admitted.remove(&player_id);
        if let Some(board) = &mut self.board
            && board.owner == Some(player_id)
        {
            board.owner = None;
        }

        Ok(true)
    }

    fn apply_command(
        &mut self,
        player_id: u32,
        sequence: u32,
        payload: &[u8],
    ) -> Result<(), SimulationError> {
        let player = self.require_admitted(player_id)?;
        if payload.len() > game_server::MAX_COMMAND_PAYLOAD_BYTES {
            return Err(SimulationError::new(
                "command exceeds runtime payload limit",
            ));
        }
        let envelope = CommandEnvelope::decode(payload).map_err(error)?;
        if let Some(board) = &mut self.board {
            match &envelope.command {
                Command::ClaimBoard { request_id } => {
                    if self.session.state().rack(player).is_some()
                        || board.owner.is_some_and(|owner| owner != player_id)
                        || !board.authority.verify(request_id)
                    {
                        return Err(SimulationError::new("board ownership proof rejected"));
                    }
                    self.session
                        .accept_board_claim(player, sequence, &envelope)
                        .map_err(error)?;
                    board.owner = Some(player_id);
                }
                Command::SetName { .. } => {
                    if board.owner == Some(player_id) {
                        return Err(SimulationError::new("board cannot occupy a playing seat"));
                    }
                    self.session
                        .join_named_player(player, sequence, &envelope)
                        .map_err(error)?;
                }
                Command::Start {} => {
                    if board.owner != Some(player_id) {
                        return Err(SimulationError::new(
                            "only the shared board can start this match",
                        ));
                    }
                    self.session
                        .start_by_authorized_board(player, sequence, &envelope)
                        .map_err(error)?;
                }
                _ => self
                    .session
                    .apply(player, sequence, &envelope)
                    .map_err(|failure| command_error(failure, &envelope))?,
            }
        } else {
            self.session
                .apply(player, sequence, &envelope)
                .map_err(|failure| command_error(failure, &envelope))?;
        }
        self.preview_deadline = match &envelope.command {
            Command::Preview { placements } if !placements.is_empty() => {
                Some((player, self.tick.saturating_add(PREVIEW_LIFETIME_TICKS)))
            }
            _ => None,
        };
        if matches!(self.session.state().phase(), Phase::Finished { .. })
            && let Some(lifecycle) = &mut self.lifecycle
            && lifecycle.finished_at_tick.is_none()
        {
            lifecycle.finished_at_tick = Some(self.tick);
        }
        Ok(())
    }

    fn advance_tick(&mut self) -> Result<(), SimulationError> {
        self.tick = self
            .tick
            .checked_add(1)
            .ok_or_else(|| SimulationError::new("simulation tick exhausted"))?;
        if let Some((player, deadline)) = self.preview_deadline
            && self.tick >= deadline
        {
            self.session.clear_preview_for(player);
            self.preview_deadline = None;
        }
        Ok(())
    }

    fn snapshot_scope(&self) -> SnapshotScope {
        SnapshotScope::PlayerScoped
    }

    fn snapshot(&self) -> Result<SimulationSnapshot, SimulationError> {
        #[derive(Serialize)]
        struct Canonical<'a> {
            version: u8,
            admitted: &'a BTreeSet<u32>,
            last_admitted_player: Option<u32>,
            game: &'a RawValue,
            #[serde(skip_serializing_if = "Option::is_none")]
            lifecycle: Option<&'a MatchLifecycle>,
            #[serde(skip_serializing_if = "Option::is_none")]
            board: Option<CanonicalBoard>,
        }
        #[derive(Serialize)]
        struct CanonicalBoard {
            owner: Option<u32>,
        }
        let game = self.session.state().canonical_bytes().map_err(error)?;
        let game: Box<RawValue> = serde_json::from_slice(&game).map_err(error)?;
        let payload = serde_json::to_vec(&Canonical {
            version: if self.board.is_some() { 2 } else { 1 },
            admitted: &self.admitted,
            last_admitted_player: self.last_admitted_player,
            game: &game,
            lifecycle: self.lifecycle.as_ref(),
            board: self
                .board
                .as_ref()
                .map(|board| CanonicalBoard { owner: board.owner }),
        })
        .map_err(error)?;
        Ok(SimulationSnapshot::new(self.tick, payload))
    }

    fn snapshot_for(&self, player_id: u32) -> Result<SimulationSnapshot, SimulationError> {
        let player = self.require_admitted(player_id)?;
        let snapshot = self.player_projection(player)?;
        let payload = serde_json::to_vec(&snapshot).map_err(error)?;
        Ok(SimulationSnapshot::new(self.tick, payload))
    }
    fn snapshot_for_with_context(
        &self,
        player_id: u32,
        context: game_server::PlayerSnapshotContext<'_>,
    ) -> Result<SimulationSnapshot, SimulationError> {
        let player = self.require_admitted(player_id)?;
        let mut snapshot = self.player_projection(player)?;
        for player in &mut snapshot.public.players {
            let id = u32::try_from(player.id.get()).map_err(error)?;
            player.connected = Some(context.is_connected(id));
        }
        if snapshot.public.preview.as_ref().is_some_and(|preview| {
            u32::try_from(preview.player_id.get())
                .map(|id| !context.is_connected(id))
                .unwrap_or(true)
        }) {
            snapshot.public.preview = None;
        }
        let payload = serde_json::to_vec(&snapshot).map_err(error)?;
        Ok(SimulationSnapshot::new(self.tick, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_and_abandoned_lifetimes_are_bounded_and_longer_than_creation_retries() {
        let lifecycle = MatchLifecycle {
            created_at: 1000,
            finished_at_tick: Some(20),
        };
        assert!(!lifecycle.can_retire(1001, 20 + FINISHED_RETENTION_TICKS - 1));
        assert!(!lifecycle.can_retire(1001, 20 + FINISHED_RETENTION_TICKS));
        assert!(lifecycle.can_retire(1300, 20 + FINISHED_RETENTION_TICKS));
        let abandoned = MatchLifecycle {
            created_at: 1000,
            finished_at_tick: None,
        };
        assert!(!abandoned.can_retire(1000 + MATCH_LIFETIME_SECONDS - 1, 0));
        assert!(abandoned.can_retire(1000 + MATCH_LIFETIME_SECONDS, 0));
    }

    #[test]
    fn lifecycle_is_canonical_replay_input_and_never_a_private_player_projection() {
        use game_server::{MatchRuntime, ReconnectToken};
        let make = || {
            ScrabbleSimulation::new(
                GameId::new(7),
                scrabble_game::ruleset::english_fixture(),
                [3; 32],
            )
            .unwrap()
            .with_lifecycle(1000)
        };
        let mut runtime = MatchRuntime::new_with_replay_capture(make(), 100);
        let lease = runtime.admit(ReconnectToken([5; 16])).unwrap();
        runtime.advance_tick().unwrap();
        let snapshot = runtime.snapshot().unwrap();
        let canonical: serde_json::Value = serde_json::from_slice(&snapshot.payload).unwrap();
        assert_eq!(canonical["lifecycle"]["created_at"], 1000);
        let private = runtime.snapshot_for(lease.player_id).unwrap();
        let private: serde_json::Value = serde_json::from_slice(&private.payload).unwrap();
        assert!(private.get("lifecycle").is_none());
        runtime.freeze_for_recovery();
        let recovered =
            MatchRuntime::restore_from_recovery(make(), runtime.recovery_image().unwrap()).unwrap();
        assert_eq!(recovered.snapshot().unwrap(), snapshot);
        assert!(
            retirement_due(
                &snapshot.payload,
                1000 + MATCH_LIFETIME_SECONDS,
                snapshot.tick
            )
            .unwrap()
        );
    }

    #[test]
    fn tick_exhaustion_is_atomic() {
        let mut simulation = ScrabbleSimulation::new(
            GameId::new(1),
            scrabble_game::ruleset::english_fixture(),
            [7; 32],
        )
        .unwrap();
        simulation.tick = u64::MAX;
        let before = simulation.snapshot().unwrap();
        assert!(simulation.advance_tick().is_err());
        assert_eq!(simulation.snapshot().unwrap(), before);
    }
}
