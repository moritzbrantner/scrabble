use game_server::{GameSimulation, SimulationError, SimulationSnapshot, SnapshotScope};
use scrabble_game::{
    identity::{GameId, PlayerId},
    protocol::{CommandEnvelope, Phase},
    ruleset::Ruleset,
    session::GameSession,
    state::{GameState, StateError},
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::BTreeSet;

pub const SCRABBLE_TICK_HZ: u16 = 20;

/// No Debug/Serialize: canonical state is private and requires an explicit snapshot call.
pub struct ScrabbleSimulation {
    session: GameSession,
    tick: u64,
    admitted: BTreeSet<u32>,
    last_admitted_player: Option<u32>,
    lifecycle: Option<MatchLifecycle>,
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

fn error(error: impl std::fmt::Display) -> SimulationError {
    SimulationError::new(error.to_string())
}

impl ScrabbleSimulation {
    pub fn new(game_id: GameId, ruleset: Ruleset, seed: [u8; 32]) -> Result<Self, StateError> {
        Ok(Self {
            session: GameSession::new(GameState::new(game_id, ruleset, seed)?),
            tick: 0,
            admitted: BTreeSet::new(),
            last_admitted_player: None,
            lifecycle: None,
        })
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
    fn tick_hz(&self) -> u16 {
        SCRABBLE_TICK_HZ
    }
    fn max_players(&self) -> usize {
        usize::from(self.session.state().ruleset().maximum_players)
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
        self.session
            .add_player(
                PlayerId::new(u64::from(player_id)),
                format!("Player {player_id}"),
            )
            .map_err(error)?;
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
            self.session.remove_lobby_player(player).map_err(error)?;
        } else {
            // Expiry never donates the rack or replaces a started game's seat.
            // Forfeit/turn resolution is the explicit policy added in #27.
            self.session.clear_preview_for(player);
        }
        self.admitted.remove(&player_id);

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
        self.session
            .apply(player, sequence, &envelope)
            .map_err(error)?;
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
        }
        let game = self.session.state().canonical_bytes().map_err(error)?;
        let game: Box<RawValue> = serde_json::from_slice(&game).map_err(error)?;
        let payload = serde_json::to_vec(&Canonical {
            version: 1,
            admitted: &self.admitted,
            last_admitted_player: self.last_admitted_player,
            game: &game,
            lifecycle: self.lifecycle.as_ref(),
        })
        .map_err(error)?;
        Ok(SimulationSnapshot::new(self.tick, payload))
    }

    fn snapshot_for(&self, player_id: u32) -> Result<SimulationSnapshot, SimulationError> {
        let player = self.require_admitted(player_id)?;
        let snapshot = self.session.player_snapshot(player).map_err(error)?;
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
