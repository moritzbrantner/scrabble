use game_server::{
    CommandOutcome, GameSimulation, MatchRuntime, ReconnectToken, ReplayLog, SnapshotScope,
    verify_replay,
};
use scrabble_game::{
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement, PlayerSnapshot},
    ruleset::{TileFace, english_fixture},
};
use scrabble_server::simulation::ScrabbleSimulation;
use serde_json::Value;
use std::collections::BTreeSet;

fn simulation() -> ScrabbleSimulation {
    ScrabbleSimulation::new(GameId::new(1), english_fixture(), [7; 32]).unwrap()
}
fn command(player: u32, sequence: u32, turn: u64, command: Command) -> Vec<u8> {
    serde_json::to_vec(&CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(u64::from(player)),
        sequence,
        expected_turn: TurnId::new(turn),
        command,
    })
    .unwrap()
}
fn projected(simulation: &ScrabbleSimulation, player: u32) -> PlayerSnapshot {
    serde_json::from_slice(&simulation.snapshot_for(player).unwrap().payload).unwrap()
}
fn started() -> ScrabbleSimulation {
    let mut simulation = simulation();
    simulation.add_player(1).unwrap();
    simulation.add_player(2).unwrap();
    simulation
        .apply_command(1, 1, &command(1, 1, 0, Command::Start {}))
        .unwrap();
    simulation
}
fn draft(snapshot: &PlayerSnapshot) -> Command {
    let tile = &snapshot.own_rack.tiles[0];
    Command::Preview {
        placements: vec![Placement {
            tile_id: tile.id,
            coordinate: Coordinate::new(7, 7).unwrap(),
            blank_as: match tile.face {
                TileFace::Letter { .. } => None,
                TileFace::Blank {} => Some('A'),
            },
        }],
    }
}

#[test]
fn canonical_authority_and_two_private_projections_are_distinct() {
    let simulation = started();
    assert_eq!(simulation.snapshot_scope(), SnapshotScope::PlayerScoped);
    let first = projected(&simulation, 1);
    let second = projected(&simulation, 2);
    assert_eq!(first.public, second.public);
    assert_eq!(first.own_rack.player_id, PlayerId::new(1));
    assert_eq!(second.own_rack.player_id, PlayerId::new(2));
    assert_eq!(first.own_rack.tiles.len(), 7);
    assert_eq!(second.own_rack.tiles.len(), 7);
    let first_ids: BTreeSet<_> = first.own_rack.tiles.iter().map(|tile| tile.id).collect();
    let second_ids: BTreeSet<_> = second.own_rack.tiles.iter().map(|tile| tile.id).collect();
    assert!(first_ids.is_disjoint(&second_ids));
    let canonical: Value = serde_json::from_slice(&simulation.snapshot().unwrap().payload).unwrap();
    assert_eq!(canonical["version"], 1);
    assert_eq!(canonical["admitted"], serde_json::json!([1, 2]));
    assert_eq!(canonical["game"]["bag"].as_array().unwrap().len(), 86);
    for (index, snapshot) in [first, second].iter().enumerate() {
        let rack = canonical["game"]["players"][index]["rack"]
            .as_array()
            .unwrap();
        let canonical_ids: BTreeSet<_> = rack
            .iter()
            .map(|tile| tile["id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            canonical_ids,
            snapshot
                .own_rack
                .tiles
                .iter()
                .map(|tile| tile.id.get().to_string())
                .collect()
        );
        let encoded: Value =
            serde_json::from_slice(&simulation.snapshot_for((index + 1) as u32).unwrap().payload)
                .unwrap();
        assert!(encoded.get("game").is_none());
        assert!(encoded.get("bag").is_none());
        assert!(encoded["public"].get("own_rack").is_none());
        assert!(
            encoded["public"]["players"]
                .as_array()
                .unwrap()
                .iter()
                .all(|player| player.get("rack").is_none())
        );
    }
    assert!(simulation.snapshot_for(999).is_err());
}

#[test]
fn rejection_is_atomic_and_only_authenticated_sequenced_commands_mutate() {
    let mut simulation = started();
    let before = simulation.snapshot().unwrap();
    let private = simulation.snapshot_for(1).unwrap();
    for (authenticated, sequence, payload) in [
        (2, 2, command(1, 2, 0, Command::Pass {})), // identity claim
        (1, 3, command(1, 2, 0, Command::Pass {})), // outer sequence
        (1, 2, command(1, 2, 99, Command::Pass {})), // turn
        (2, 1, command(2, 1, 0, Command::Pass {})), // inactive
        (99, 1, command(99, 1, 0, Command::Pass {})), // not admitted
        (1, 2, b"{\"version\":99}".to_vec()),
        (1, 2, vec![b' '; 1025]),
    ] {
        assert!(
            simulation
                .apply_command(authenticated, sequence, &payload)
                .is_err()
        );
        assert_eq!(simulation.snapshot().unwrap(), before);
        assert_eq!(simulation.snapshot_for(1).unwrap(), private);
    }
    let preview = draft(&projected(&simulation, 1));
    simulation
        .apply_command(1, 2, &command(1, 2, 0, preview))
        .unwrap();
    assert!(projected(&simulation, 2).public.preview.is_some());
    assert_eq!(simulation.snapshot().unwrap(), before); // preview is ephemeral
    simulation
        .apply_command(1, 3, &command(1, 3, 0, Command::Pass {}))
        .unwrap();
    assert_eq!(
        projected(&simulation, 1).public.phase,
        Phase::Playing {
            active_player: PlayerId::new(2),
            turn: TurnId::new(1)
        }
    );
    assert!(projected(&simulation, 2).public.preview.is_none());
}

#[test]
fn ticks_do_not_change_turns_and_expiry_preserves_started_seats_and_racks() {
    let mut simulation = started();
    let first = projected(&simulation, 1);
    let second = projected(&simulation, 2);
    for _ in 0..100 {
        simulation.advance_tick().unwrap();
    }
    assert_eq!(projected(&simulation, 1), first);
    assert_eq!(simulation.current_tick(), 100);
    simulation
        .apply_command(1, 2, &command(1, 2, 0, draft(&first)))
        .unwrap();
    assert!(simulation.try_remove_player(1).unwrap());
    assert!(!simulation.try_remove_player(1).unwrap());
    assert!(simulation.snapshot_for(1).is_err());
    assert!(
        simulation
            .apply_command(1, 3, &command(1, 3, 0, Command::Pass {}))
            .is_err()
    );
    assert_eq!(projected(&simulation, 2), second);
    let canonical: Value = serde_json::from_slice(&simulation.snapshot().unwrap().payload).unwrap();
    assert_eq!(canonical["last_admitted_player"], 2);
    assert_eq!(canonical["admitted"], serde_json::json!([2]));
    assert_eq!(
        canonical["game"]["players"][0]["rack"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert!(simulation.add_player(1).is_err());
    assert!(simulation.add_player(3).is_err());
}

#[test]
fn lobby_expiry_frees_capacity_without_reusing_identity_or_dealing_to_a_ghost() {
    let mut simulation = simulation();
    for id in 1..=4 {
        simulation.add_player(id).unwrap();
    }
    let before = simulation.snapshot().unwrap();
    assert!(simulation.add_player(5).is_err());
    assert_eq!(simulation.snapshot().unwrap(), before);
    assert!(simulation.try_remove_player(1).unwrap());
    assert!(simulation.add_player(1).is_err());
    simulation.add_player(5).unwrap();
    let public = projected(&simulation, 2).public;
    assert_eq!(
        public
            .players
            .iter()
            .map(|player| player.id.get())
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    simulation
        .apply_command(2, 1, &command(2, 1, 0, Command::Start {}))
        .unwrap();
    assert!(simulation.snapshot_for(1).is_err());
    assert_eq!(projected(&simulation, 2).public.remaining_tiles, 72);
    let before = simulation.snapshot().unwrap();
    assert!(simulation.add_player(6).is_err());
    assert_eq!(simulation.snapshot().unwrap(), before);
}

#[test]
fn runtime_replay_recovery_and_reconnect_use_identical_authority() {
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation(), 2);
    let first = runtime.admit(ReconnectToken([1; 16])).unwrap();
    let second = runtime.admit(ReconnectToken([2; 16])).unwrap();
    let start = command(first.player_id, 1, 0, Command::Start {});
    assert_eq!(
        runtime
            .submit_command(first.player_id, first.connection_epoch, 1, &start)
            .unwrap(),
        CommandOutcome::Applied
    );
    runtime.advance_tick().unwrap();
    assert!(runtime.disconnect(first.player_id, first.connection_epoch));
    let reconnected = runtime
        .reconnect(first.reconnect_token, ReconnectToken([3; 16]))
        .unwrap();
    assert_eq!(reconnected.player_id, first.player_id);
    let pass = command(first.player_id, 2, 0, Command::Pass {});
    assert!(
        runtime
            .submit_command(first.player_id, first.connection_epoch, 2, &pass)
            .is_err()
    );
    runtime
        .submit_command(
            reconnected.player_id,
            reconnected.connection_epoch,
            2,
            &pass,
        )
        .unwrap();
    runtime.advance_tick().unwrap();
    runtime
        .submit_command(
            second.player_id,
            second.connection_epoch,
            1,
            &command(second.player_id, 1, 1, Command::Pass {}),
        )
        .unwrap();
    runtime.advance_tick().unwrap();
    let expected = runtime.snapshot().unwrap();
    let encoded = runtime.replay_log().unwrap().encode().unwrap();
    let log = ReplayLog::decode(&encoded).unwrap();
    let verification = verify_replay(simulation(), &log).unwrap();
    assert_eq!(verification.final_snapshot, expected);
    runtime.freeze_for_recovery();
    let image = runtime.recovery_image().unwrap();
    let restored = MatchRuntime::restore_from_recovery(simulation(), image).unwrap();
    assert_eq!(restored.snapshot().unwrap(), expected);
    for id in [1, 2] {
        assert_eq!(
            restored.snapshot_for(id).unwrap(),
            runtime.snapshot_for(id).unwrap()
        );
    }
}

#[test]
fn runtime_grace_expiry_and_new_admission_replay_without_identity_substitution() {
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation(), 2);
    let departed = runtime.admit(ReconnectToken([1; 16])).unwrap();
    let host = runtime.admit(ReconnectToken([2; 16])).unwrap();
    runtime.admit(ReconnectToken([3; 16])).unwrap();
    runtime.admit(ReconnectToken([4; 16])).unwrap();
    assert!(runtime.disconnect(departed.player_id, departed.connection_epoch));
    for _ in 0..4 {
        runtime.advance_tick().unwrap();
    }
    assert!(runtime.snapshot_for(departed.player_id).is_err());
    let replacement = runtime.admit(ReconnectToken([5; 16])).unwrap();
    assert_eq!(replacement.player_id, 5);
    runtime
        .submit_command(
            host.player_id,
            host.connection_epoch,
            1,
            &command(host.player_id, 1, 0, Command::Start {}),
        )
        .unwrap();
    runtime.advance_tick().unwrap();
    let public: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(host.player_id).unwrap().payload).unwrap();
    assert_eq!(
        public
            .public
            .players
            .iter()
            .map(|player| player.id.get())
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    assert_eq!(public.public.remaining_tiles, 72);
    let replay = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(simulation(), &replay).unwrap().final_snapshot,
        runtime.snapshot().unwrap()
    );
}

#[test]
fn repeated_lobby_admission_does_not_accumulate_retired_player_storage() {
    let mut simulation = simulation();
    let before_length = simulation.snapshot().unwrap().payload.len();
    for id in 1..=1000 {
        simulation.add_player(id).unwrap();
        assert!(simulation.try_remove_player(id).unwrap());
    }
    let final_snapshot = simulation.snapshot().unwrap();
    assert!(final_snapshot.payload.len() < before_length + 20);
    let value: Value = serde_json::from_slice(&final_snapshot.payload).unwrap();
    assert_eq!(value["admitted"], serde_json::json!([]));
    assert_eq!(value["last_admitted_player"], 1000);
    assert!(simulation.add_player(999).is_err());
    assert_eq!(simulation.snapshot().unwrap(), final_snapshot);
}
