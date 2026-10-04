//! A real private Scrabble projection must fit the runtime's recovery budget over its supported lifetime.
use game_server::{MatchRuntime, recovery::MAX_RECOVERY_IMAGE_BYTES};
use scrabble_game::{identity::GameId, ruleset::english_fixture};
use scrabble_server::simulation::{MATCH_LIFETIME_SECONDS, SCRABBLE_TICK_HZ, ScrabbleSimulation};

fn recovery_size(ticks: usize) -> usize {
    let simulation = ScrabbleSimulation::new(GameId::new(7), english_fixture(), [7; 32])
        .unwrap()
        .with_lifecycle(1000);
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation, 1200);
    for _ in 0..ticks {
        runtime.advance_tick().unwrap();
    }
    runtime.freeze_for_recovery();
    runtime.recovery_image().unwrap().encode().unwrap().len()
}
#[test]
fn six_hour_idle_scrabble_history_fits_the_runtime_recovery_budget() {
    let first = recovery_size(1);
    let hundred = recovery_size(100);
    let bytes_per_tick = (hundred - first).div_ceil(99);
    let total_ticks = MATCH_LIFETIME_SECONDS * u64::from(SCRABBLE_TICK_HZ);
    let projected = first as u64 + bytes_per_tick as u64 * (total_ticks - 1);
    assert!(
        projected < MAX_RECOVERY_IMAGE_BYTES as u64,
        "idle replay grows by {bytes_per_tick} bytes per tick: {projected} bytes over the supported lifetime exceeds the {MAX_RECOVERY_IMAGE_BYTES}-byte recovery limit"
    );
}
