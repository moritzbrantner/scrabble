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

#[test]
fn recovery_rejects_previous_game_evidence_before_replaying_changed_turn_semantics() {
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation(), 100);
    let current = runtime.snapshot().unwrap();
    let old = String::from_utf8(current.payload)
        .unwrap()
        .replacen("\"game\":{\"version\":3", "\"game\":{\"version\":2", 1)
        .replacen(",\"history\":[]", "", 1);
    let value: Value = serde_json::from_str(&old).unwrap();
    assert_eq!(value["game"]["version"], 2);
    assert!(value["game"].get("history").is_none());
    let old = game_server::SimulationSnapshot::new(0, old.into_bytes());
    // A valid full checkpoint in the public native replay format, using the old game payload.
    let mut encoded = b"GSRP\x01\x04".to_vec();
    encoded.extend_from_slice(&0_u64.to_be_bytes());
    encoded.extend_from_slice(&(12 + old.payload.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&old.state_hash.to_be_bytes());
    encoded.extend_from_slice(&(old.payload.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&old.payload);
    let log = ReplayLog::decode(&encoded).unwrap();
    assert!(matches!(
        verify_replay(simulation(), &log),
        Err(game_server::ReplayError::CheckpointMismatch { .. })
    ));
    runtime.freeze_for_recovery();
    let mut image = runtime.recovery_image().unwrap();
    image.replay = log;
    assert!(MatchRuntime::restore_from_recovery(simulation(), image).is_err());
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

#[test]
fn finished_scores_are_private_projection_safe_and_survive_replay_and_recovery() {
    let fresh = || simulation().with_lifecycle(1000);
    let mut runtime = MatchRuntime::new_with_replay_capture(fresh(), 100);
    runtime.admit(ReconnectToken([1; 16])).unwrap();
    runtime.admit(ReconnectToken([2; 16])).unwrap();
    runtime
        .submit_command(1, 1, 1, &command(1, 1, 0, Command::Start {}))
        .unwrap();
    let read = |runtime: &MatchRuntime<ScrabbleSimulation>, player| -> PlayerSnapshot {
        serde_json::from_slice(&runtime.snapshot_for(player).unwrap().payload).unwrap()
    };
    let before = [read(&runtime, 1), read(&runtime, 2)];
    let rules = english_fixture();
    let remaining_values: Vec<i32> = before
        .iter()
        .map(|snapshot| {
            snapshot
                .own_rack
                .tiles
                .iter()
                .map(|tile| {
                    i32::from(
                        rules
                            .tiles
                            .iter()
                            .find(|definition| definition.face == tile.face)
                            .unwrap()
                            .value,
                    )
                })
                .sum()
        })
        .collect();
    for turn in 0..u64::from(rules.scoreless_turn_limit) {
        let player = (turn % 2 + 1) as u32;
        let sequence = (turn / 2 + if player == 1 { 2 } else { 1 }) as u32;
        runtime
            .submit_command(
                player,
                1,
                sequence,
                &command(player, sequence, turn, Command::Pass {}),
            )
            .unwrap();
        runtime.advance_tick().unwrap();
    }
    let after = [read(&runtime, 1), read(&runtime, 2)];
    assert_eq!(after[0].public, after[1].public);
    for index in 0..2 {
        assert_eq!(after[index].own_rack, before[index].own_rack);
        assert_eq!(
            after[0].public.players[index].score,
            -remaining_values[index]
        );
    }
    let highest = remaining_values.iter().min().unwrap();
    assert_eq!(
        after[0].public.phase,
        Phase::Finished {
            winners: remaining_values
                .iter()
                .enumerate()
                .filter_map(
                    |(index, value)| (value == highest).then_some(PlayerId::new(index as u64 + 1))
                )
                .collect(),
        }
    );
    let public = serde_json::to_value(&after[0].public).unwrap();
    assert!(public.get("consecutive_scoreless_turns").is_none());
    assert!(public.get("bag").is_none());
    assert!(
        public["players"]
            .as_array()
            .unwrap()
            .iter()
            .all(|player| player.get("rack").is_none())
    );
    let canonical = runtime.snapshot().unwrap();
    let value: Value = serde_json::from_slice(&canonical.payload).unwrap();
    assert_eq!(value["game"]["version"], 3);
    assert_eq!(value["game"]["consecutive_scoreless_turns"], 6);
    assert_eq!(value["lifecycle"]["finished_at_tick"], 5);
    let records = runtime.replay_log().unwrap().records().len();
    assert!(
        runtime
            .submit_command(1, 1, 5, &command(1, 5, 6, Command::Pass {}))
            .is_err()
    );
    assert_eq!(runtime.snapshot().unwrap(), canonical);
    assert_eq!(runtime.replay_log().unwrap().records().len(), records);
    let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(fresh(), &log).unwrap().final_snapshot,
        canonical
    );
    runtime.freeze_for_recovery();
    let mut recovered =
        MatchRuntime::restore_from_recovery(fresh(), runtime.recovery_image().unwrap()).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), canonical);
    let mut offline = after.clone();
    for snapshot in &mut offline {
        for player in &mut snapshot.public.players {
            player.connected = Some(false);
        }
    }
    assert_eq!(read(&recovered, 1), offline[0]);
    assert_eq!(read(&recovered, 2), offline[1]);
    recovered
        .reconnect(ReconnectToken([1; 16]), ReconnectToken([3; 16]))
        .unwrap();
    assert!(
        recovered
            .submit_command(1, 2, 5, &command(1, 5, 6, Command::Pass {}))
            .is_err()
    );
    assert_eq!(read(&recovered, 1).public.phase, after[0].public.phase);
    recovered
        .reconnect(ReconnectToken([2; 16]), ReconnectToken([4; 16]))
        .unwrap();
    assert_eq!(read(&recovered, 1).public.players, after[0].public.players);
}

#[test]
fn empty_bag_go_out_final_scores_replay_and_recover_exactly() {
    use scrabble_game::{dictionary::WordList, ruleset::TileDefinition};
    use std::sync::Arc;
    let mut rules = english_fixture();
    rules.maximum_players = 2;
    rules.rack_size = 2;
    rules.bingo_bonus = 0;
    rules.tiles = vec![TileDefinition {
        face: TileFace::Letter { letter: 'A' },
        count: 4,
        value: 1,
    }];
    let dictionary = Arc::new(WordList::from_text(rules.dictionary.clone(), "AA\n").unwrap());
    let fresh = || {
        ScrabbleSimulation::with_dictionary(
            GameId::new(1),
            rules.clone(),
            [7; 32],
            dictionary.clone(),
        )
        .unwrap()
    };
    let mut runtime = MatchRuntime::new_with_replay_capture(fresh(), 100);
    runtime.admit(ReconnectToken([1; 16])).unwrap();
    runtime.admit(ReconnectToken([2; 16])).unwrap();
    runtime
        .submit_command(1, 1, 1, &command(1, 1, 0, Command::Start {}))
        .unwrap();
    let own: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    let placements = own
        .own_rack
        .tiles
        .iter()
        .enumerate()
        .map(|(index, tile)| Placement {
            tile_id: tile.id,
            coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
            blank_as: None,
        })
        .collect();
    runtime
        .submit_command(1, 1, 2, &command(1, 2, 0, Command::Commit { placements }))
        .unwrap();
    let finished: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    assert_eq!(
        finished.public.phase,
        Phase::Finished {
            winners: vec![PlayerId::new(1)]
        }
    );
    assert_eq!(
        finished
            .public
            .players
            .iter()
            .map(|player| player.score)
            .collect::<Vec<_>>(),
        vec![6, -2]
    );
    assert_eq!(finished.public.history.len(), 1);
    assert_eq!(
        finished.public.history[0].action,
        scrabble_game::protocol::TurnAction::Commit {
            words: vec!["AA".into()],
            move_score: 4,
            blank_count: 0,
        }
    );
    assert_eq!(
        finished.public.history[0]
            .scores
            .iter()
            .map(|score| score.delta)
            .collect::<Vec<_>>(),
        vec![6, -2]
    );
    assert!(finished.own_rack.tiles.is_empty());
    let canonical = runtime.snapshot().unwrap();
    let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(fresh(), &log).unwrap().final_snapshot,
        canonical
    );
    runtime.freeze_for_recovery();
    let recovered =
        MatchRuntime::restore_from_recovery(fresh(), runtime.recovery_image().unwrap()).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), canonical);
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
    runtime
        .submit_command(
            second.player_id,
            second.connection_epoch,
            1,
            &command(
                second.player_id,
                1,
                0,
                Command::SetName {
                    display_name: "Player 1".into(),
                },
            ),
        )
        .unwrap();
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
            2,
            &command(second.player_id, 2, 1, Command::Pass {}),
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
        let mut expected: PlayerSnapshot =
            serde_json::from_slice(&runtime.snapshot_for(id).unwrap().payload).unwrap();
        assert!(
            expected
                .public
                .players
                .iter()
                .all(|player| player.connected == Some(true))
        );
        for player in &mut expected.public.players {
            player.connected = Some(false);
        }
        let recovered: PlayerSnapshot =
            serde_json::from_slice(&restored.snapshot_for(id).unwrap().payload).unwrap();
        assert_eq!(recovered, expected);
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
fn expired_started_capability_cannot_resume_or_control_retained_tiles() {
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation(), 2);
    let first = runtime.admit(ReconnectToken([1; 16])).unwrap();
    runtime.admit(ReconnectToken([2; 16])).unwrap();
    runtime
        .submit_command(1, 1, 1, &command(1, 1, 0, Command::Start {}))
        .unwrap();
    let before: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(2).unwrap().payload).unwrap();
    assert!(runtime.disconnect(1, first.connection_epoch));
    for _ in 0..4 {
        runtime.advance_tick().unwrap();
    }
    let after: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(2).unwrap().payload).unwrap();
    assert_eq!(after.own_rack, before.own_rack);
    assert_eq!(after.public.phase, before.public.phase);
    assert_eq!(after.public.board, before.public.board);
    assert_eq!(after.public.remaining_tiles, before.public.remaining_tiles);
    assert_eq!(
        after
            .public
            .players
            .iter()
            .map(|player| (player.id, player.score, player.rack_count))
            .collect::<Vec<_>>(),
        before
            .public
            .players
            .iter()
            .map(|player| (player.id, player.score, player.rack_count))
            .collect::<Vec<_>>()
    );
    let canonical = runtime.snapshot().unwrap();
    let records = runtime.replay_log().unwrap().records().len();
    assert!(
        runtime
            .reconnect(first.reconnect_token, ReconnectToken([3; 16]))
            .is_err()
    );
    assert!(runtime.snapshot_for(1).is_err());
    assert!(
        runtime
            .submit_command(
                1,
                first.connection_epoch,
                2,
                &command(1, 2, 0, Command::Pass {})
            )
            .is_err()
    );
    assert_eq!(runtime.snapshot().unwrap(), canonical);
    assert_eq!(runtime.replay_log().unwrap().records().len(), records);
    assert_eq!(
        verify_replay(simulation(), runtime.replay_log().unwrap())
            .unwrap()
            .final_snapshot,
        canonical
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

#[test]
fn preview_expiry_clears_lost_cancel_without_changing_gameplay() {
    use scrabble_server::simulation::PREVIEW_LIFETIME_TICKS;
    let mut previewing = started();
    let mut untouched = started();
    let first = projected(&previewing, 1);
    previewing
        .apply_command(1, 2, &command(1, 2, 0, draft(&first)))
        .unwrap();
    let preview = projected(&previewing, 2).public.preview.unwrap();
    assert_eq!(preview.tiles.len(), 1);
    assert_eq!(preview.player_id, PlayerId::new(1));
    assert_eq!(preview.turn, TurnId::new(0));
    for _ in 0..PREVIEW_LIFETIME_TICKS - 1 {
        previewing.advance_tick().unwrap();
        untouched.advance_tick().unwrap();
    }
    assert!(projected(&previewing, 2).public.preview.is_some());
    previewing.advance_tick().unwrap();
    untouched.advance_tick().unwrap();
    assert!(projected(&previewing, 2).public.preview.is_none());
    assert_eq!(
        previewing.snapshot().unwrap(),
        untouched.snapshot().unwrap()
    );
    assert_eq!(projected(&previewing, 1), projected(&untouched, 1));
}

#[test]
fn preview_packets_are_sequenced_and_disconnect_resume_cannot_resurrect_them() {
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation(), 100);
    let first = runtime.admit(ReconnectToken([1; 16])).unwrap();
    let second = runtime.admit(ReconnectToken([2; 16])).unwrap();
    runtime
        .submit_command(
            first.player_id,
            first.connection_epoch,
            1,
            &command(1, 1, 0, Command::Start {}),
        )
        .unwrap();
    let read = |runtime: &MatchRuntime<ScrabbleSimulation>, player| -> PlayerSnapshot {
        serde_json::from_slice(&runtime.snapshot_for(player).unwrap().payload).unwrap()
    };
    let placement = draft(&read(&runtime, 1));
    let payload = command(1, 2, 0, placement.clone());
    let canonical = runtime.snapshot().unwrap();
    runtime
        .submit_command(1, first.connection_epoch, 2, &payload)
        .unwrap();
    assert_eq!(read(&runtime, 2).public.preview.unwrap().tiles.len(), 1);
    runtime
        .submit_command(
            1,
            first.connection_epoch,
            3,
            &command(1, 3, 0, Command::Preview { placements: vec![] }),
        )
        .unwrap();
    assert_eq!(
        runtime
            .submit_command(1, first.connection_epoch, 2, &payload)
            .unwrap(),
        CommandOutcome::IgnoredStale
    );
    assert!(read(&runtime, 2).public.preview.is_none());
    runtime
        .submit_command(
            1,
            first.connection_epoch,
            4,
            &command(1, 4, 0, placement.clone()),
        )
        .unwrap();
    assert!(!runtime.disconnect(1, first.connection_epoch + 1));
    assert!(read(&runtime, 2).public.preview.is_some());
    assert!(runtime.disconnect(1, first.connection_epoch));
    assert!(read(&runtime, 2).public.preview.is_none());
    let resumed = runtime
        .reconnect(first.reconnect_token, ReconnectToken([3; 16]))
        .unwrap();
    assert!(read(&runtime, 2).public.preview.is_none());
    assert!(
        runtime
            .submit_command(
                1,
                first.connection_epoch,
                5,
                &command(1, 5, 0, placement.clone())
            )
            .is_err()
    );
    runtime
        .submit_command(1, resumed.connection_epoch, 5, &command(1, 5, 0, placement))
        .unwrap();
    assert!(read(&runtime, 2).public.preview.is_some());
    // The runtime rejects reconnect while the current lease is still connected.
    assert!(
        runtime
            .reconnect(resumed.reconnect_token, ReconnectToken([4; 16]))
            .is_err()
    );
    assert!(read(&runtime, 2).public.preview.is_some());
    assert!(runtime.disconnect(1, resumed.connection_epoch));
    runtime
        .reconnect(resumed.reconnect_token, ReconnectToken([4; 16]))
        .unwrap();
    assert!(read(&runtime, 2).public.preview.is_none());
    assert_eq!(runtime.snapshot().unwrap(), canonical);
    assert_eq!(second.player_id, 2);
}

#[test]
fn stale_turn_and_forged_rack_preview_cannot_replace_active_preview() {
    let mut simulation = started();
    let first = projected(&simulation, 1);
    simulation
        .apply_command(1, 2, &command(1, 2, 0, Command::Pass {}))
        .unwrap();
    let second = projected(&simulation, 2);
    simulation
        .apply_command(2, 1, &command(2, 1, 1, draft(&second)))
        .unwrap();
    let before = simulation.snapshot_for(2).unwrap();
    for (player, sequence, turn, preview) in [
        (1, 3, 0, draft(&first)),
        (1, 3, 1, draft(&first)),
        (2, 2, 1, draft(&first)),
    ] {
        assert!(
            simulation
                .apply_command(player, sequence, &command(player, sequence, turn, preview))
                .is_err()
        );
        assert_eq!(simulation.snapshot_for(2).unwrap(), before);
    }
    let preview = projected(&simulation, 1).public.preview.unwrap();
    assert_eq!(preview.player_id, PlayerId::new(2));
    assert_eq!(preview.turn, TurnId::new(1));
    assert_eq!(preview.tiles.len(), 1);
}

#[test]
fn replay_recovery_hides_offline_preview_and_resume_clears_it() {
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation(), 100);
    let first = runtime.admit(ReconnectToken([1; 16])).unwrap();
    runtime.admit(ReconnectToken([2; 16])).unwrap();
    runtime
        .submit_command(
            1,
            first.connection_epoch,
            1,
            &command(1, 1, 0, Command::Start {}),
        )
        .unwrap();
    let read = |runtime: &MatchRuntime<ScrabbleSimulation>| -> PlayerSnapshot {
        serde_json::from_slice(&runtime.snapshot_for(2).unwrap().payload).unwrap()
    };
    let own: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    runtime
        .submit_command(1, first.connection_epoch, 2, &command(1, 2, 0, draft(&own)))
        .unwrap();
    assert!(read(&runtime).public.preview.is_some());
    let canonical = runtime.snapshot().unwrap();
    runtime.freeze_for_recovery();
    let mut restored =
        MatchRuntime::restore_from_recovery(simulation(), runtime.recovery_image().unwrap())
            .unwrap();
    assert!(read(&restored).public.preview.is_none());
    restored
        .reconnect(first.reconnect_token, ReconnectToken([3; 16]))
        .unwrap();
    assert!(read(&restored).public.preview.is_none());
    assert_eq!(restored.snapshot().unwrap(), canonical);
}

#[test]
fn committed_turn_converges_privately_and_replays_with_exactly_once_scoring() {
    let (seed, mut runtime, placements) = (0..=255)
        .find_map(|seed| {
            let simulation =
                ScrabbleSimulation::new(GameId::new(1), english_fixture(), [seed; 32]).unwrap();
            let mut runtime = MatchRuntime::new_with_replay_capture(simulation, 100);
            runtime.admit(ReconnectToken([1; 16])).unwrap();
            runtime.admit(ReconnectToken([2; 16])).unwrap();
            runtime
                .submit_command(1, 1, 1, &command(1, 1, 0, Command::Start {}))
                .unwrap();
            let own: PlayerSnapshot =
                serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
            let placements: Option<Vec<_>> = ['A', 'T']
                .into_iter()
                .enumerate()
                .map(|(index, letter)| {
                    own.own_rack
                        .tiles
                        .iter()
                        .find(|tile| tile.face == (TileFace::Letter { letter }))
                        .map(|tile| Placement {
                            tile_id: tile.id,
                            coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
                            blank_as: None,
                        })
                })
                .collect();
            placements.map(|placements| (seed, runtime, placements))
        })
        .expect("deterministic AT fixture");
    runtime
        .submit_command(
            1,
            1,
            2,
            &command(
                1,
                2,
                0,
                Command::Preview {
                    placements: placements.clone(),
                },
            ),
        )
        .unwrap();
    let mut invalid = placements.clone();
    invalid[0].coordinate = placements[1].coordinate;
    invalid[1].coordinate = placements[0].coordinate;
    let before = runtime.snapshot().unwrap();
    let first_before = runtime.snapshot_for(1).unwrap();
    let second_before = runtime.snapshot_for(2).unwrap();
    let records_before = runtime.replay_log().unwrap().records().len();
    let failure = runtime
        .submit_command(
            1,
            1,
            3,
            &command(
                1,
                3,
                0,
                Command::Commit {
                    placements: invalid,
                },
            ),
        )
        .unwrap_err();
    let game_server::RuntimeError::Simulation(failure) = failure else {
        panic!("expected a dictionary rejection");
    };
    let feedback: Value = serde_json::from_slice(failure.command_rejection().unwrap()).unwrap();
    assert_eq!(
        feedback,
        serde_json::json!({ "version": 1, "game_id": "1", "expected_turn": "0", "error": { "kind": "invalid_words", "words": ["TA"] } })
    );
    assert!(!format!("{failure:?} {failure}").contains("TA"));
    assert_eq!(runtime.snapshot().unwrap(), before);
    assert_eq!(runtime.snapshot_for(1).unwrap(), first_before);
    assert_eq!(runtime.snapshot_for(2).unwrap(), second_before);
    assert_eq!(
        runtime.replay_log().unwrap().records().len(),
        records_before
    );
    let payload = command(1, 3, 0, Command::Commit { placements });
    assert_eq!(
        runtime.submit_command(1, 1, 3, &payload).unwrap(),
        CommandOutcome::Applied
    );
    let committed = runtime.snapshot().unwrap();
    assert_eq!(
        runtime.submit_command(1, 1, 3, &payload).unwrap(),
        CommandOutcome::IgnoredStale
    );
    assert_eq!(runtime.snapshot().unwrap(), committed);
    let first: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    let second: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(2).unwrap().payload).unwrap();
    assert_eq!(first.public, second.public);
    assert_eq!(first.public.players[0].score, 4);
    assert_eq!(first.public.board.len(), 2);
    assert!(first.public.preview.is_none());
    assert_eq!(
        first.public.phase,
        Phase::Playing {
            active_player: PlayerId::new(2),
            turn: TurnId::new(1)
        }
    );
    assert_eq!(first.own_rack.tiles.len(), 7);
    assert_eq!(first.public.remaining_tiles, 84);
    assert_eq!(second.own_rack.tiles.len(), 7);
    assert!(runtime.disconnect(1, 1));
    let tile = &second.own_rack.tiles[0];
    runtime
        .submit_command(
            2,
            1,
            1,
            &command(
                2,
                1,
                1,
                Command::Preview {
                    placements: vec![Placement {
                        tile_id: tile.id,
                        coordinate: Coordinate::new(6, 7).unwrap(),
                        blank_as: match tile.face {
                            TileFace::Blank {} => Some('A'),
                            TileFace::Letter { .. } => None,
                        },
                    }],
                },
            ),
        )
        .unwrap();
    let resumed = runtime
        .reconnect(ReconnectToken([1; 16]), ReconnectToken([3; 16]))
        .unwrap();
    assert_eq!(resumed.player_id, 1);
    let resumed_projection: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    assert_eq!(resumed_projection.own_rack, first.own_rack);
    runtime.advance_tick().unwrap();
    let fresh = || ScrabbleSimulation::new(GameId::new(1), english_fixture(), [seed; 32]).unwrap();
    let expected = runtime.snapshot().unwrap();
    let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(fresh(), &log).unwrap().final_snapshot,
        expected
    );
    runtime.freeze_for_recovery();
    let mut restored =
        MatchRuntime::restore_from_recovery(fresh(), runtime.recovery_image().unwrap()).unwrap();
    assert_eq!(restored.snapshot().unwrap(), expected);
    restored
        .reconnect(ReconnectToken([3; 16]), ReconnectToken([4; 16]))
        .unwrap();
    let restored_projection: PlayerSnapshot =
        serde_json::from_slice(&restored.snapshot_for(1).unwrap().payload).unwrap();
    assert_eq!(restored_projection.own_rack, first.own_rack);
}

#[test]
fn pass_is_fenced_exactly_once_and_replays_without_changing_tile_ownership() {
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation(), 100);
    runtime.admit(ReconnectToken([1; 16])).unwrap();
    runtime.admit(ReconnectToken([2; 16])).unwrap();
    runtime
        .submit_command(1, 1, 1, &command(1, 1, 0, Command::Start {}))
        .unwrap();
    let own: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    runtime
        .submit_command(1, 1, 2, &command(1, 2, 0, draft(&own)))
        .unwrap();
    let before = runtime.snapshot().unwrap();
    let private = runtime.snapshot_for(1).unwrap();
    for (player, sequence, turn) in [(2, 1, 0), (1, 3, 99)] {
        assert!(
            runtime
                .submit_command(
                    player,
                    1,
                    sequence,
                    &command(player, sequence, turn, Command::Pass {})
                )
                .is_err()
        );
        assert_eq!(runtime.snapshot().unwrap(), before);
        assert_eq!(runtime.snapshot_for(1).unwrap(), private);
    }
    let payload = command(1, 3, 0, Command::Pass {});
    assert_eq!(
        runtime.submit_command(1, 1, 3, &payload).unwrap(),
        CommandOutcome::Applied
    );
    let passed = runtime.snapshot().unwrap();
    assert_eq!(
        runtime.submit_command(1, 1, 3, &payload).unwrap(),
        CommandOutcome::IgnoredStale
    );
    for turn in [0, 1] {
        assert!(
            runtime
                .submit_command(1, 1, 4, &command(1, 4, turn, Command::Pass {}))
                .is_err()
        );
        assert_eq!(runtime.snapshot().unwrap(), passed);
    }
    let projection: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    assert_eq!(
        projection.public.phase,
        Phase::Playing {
            active_player: PlayerId::new(2),
            turn: TurnId::new(1)
        }
    );
    assert!(projection.public.preview.is_none());
    assert_eq!(projection.own_rack, own.own_rack);
    let before: Value = serde_json::from_slice(&before.payload).unwrap();
    let after: Value = serde_json::from_slice(&passed.payload).unwrap();
    for field in ["board", "bag", "players"] {
        assert_eq!(after["game"][field], before["game"][field]);
    }
    runtime
        .submit_command(2, 1, 1, &command(2, 1, 1, Command::Pass {}))
        .unwrap();
    runtime.advance_tick().unwrap();
    let expected = runtime.snapshot().unwrap();
    let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(simulation(), &log).unwrap().final_snapshot,
        expected
    );
    runtime.freeze_for_recovery();
    let restored =
        MatchRuntime::restore_from_recovery(simulation(), runtime.recovery_image().unwrap())
            .unwrap();
    assert_eq!(restored.snapshot().unwrap(), expected);
}

#[test]
fn exchanged_racks_remain_private_and_replay_through_recovery() {
    let mut runtime = MatchRuntime::new_with_replay_capture(simulation(), 100);
    runtime.admit(ReconnectToken([1; 16])).unwrap();
    runtime.admit(ReconnectToken([2; 16])).unwrap();
    runtime
        .submit_command(1, 1, 1, &command(1, 1, 0, Command::Start {}))
        .unwrap();
    let own: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    let other: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(2).unwrap().payload).unwrap();
    runtime
        .submit_command(1, 1, 2, &command(1, 2, 0, draft(&own)))
        .unwrap();
    let before = runtime.snapshot().unwrap();
    let private = runtime.snapshot_for(1).unwrap();
    let ids: Vec<_> = own
        .own_rack
        .tiles
        .iter()
        .take(2)
        .map(|tile| tile.id)
        .collect();
    for (player, sequence, turn, selected) in [
        (2, 1, 0, vec![other.own_rack.tiles[0].id]),
        (1, 3, 99, ids.clone()),
        (1, 3, 0, vec![other.own_rack.tiles[0].id]),
        (1, 3, 0, vec![ids[0], ids[0]]),
    ] {
        assert!(
            runtime
                .submit_command(
                    player,
                    1,
                    sequence,
                    &command(
                        player,
                        sequence,
                        turn,
                        Command::Exchange { tile_ids: selected }
                    )
                )
                .is_err()
        );
        assert_eq!(runtime.snapshot().unwrap(), before);
        assert_eq!(runtime.snapshot_for(1).unwrap(), private);
    }
    let payload = command(
        1,
        3,
        0,
        Command::Exchange {
            tile_ids: ids.clone(),
        },
    );
    assert_eq!(
        runtime.submit_command(1, 1, 3, &payload).unwrap(),
        CommandOutcome::Applied
    );
    let exchanged = runtime.snapshot().unwrap();
    assert_eq!(
        runtime.submit_command(1, 1, 3, &payload).unwrap(),
        CommandOutcome::IgnoredStale
    );
    assert!(
        runtime
            .submit_command(
                1,
                1,
                4,
                &command(
                    1,
                    4,
                    0,
                    Command::Exchange {
                        tile_ids: ids.clone()
                    }
                )
            )
            .is_err()
    );
    assert_eq!(runtime.snapshot().unwrap(), exchanged);
    let first: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(1).unwrap().payload).unwrap();
    let second: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(2).unwrap().payload).unwrap();
    assert_eq!(first.public, second.public);
    assert_eq!(second.own_rack, other.own_rack);
    assert_eq!(first.own_rack.tiles.len(), 7);
    assert_eq!(&first.own_rack.tiles[..5], &own.own_rack.tiles[2..]);
    assert!(
        first
            .own_rack
            .tiles
            .iter()
            .all(|tile| !ids.contains(&tile.id))
    );
    assert_eq!(first.public.board, own.public.board);
    assert_eq!(first.public.players, own.public.players);
    assert_eq!(first.public.remaining_tiles, own.public.remaining_tiles);
    assert!(first.public.preview.is_none());
    assert_eq!(
        first.public.phase,
        Phase::Playing {
            active_player: PlayerId::new(2),
            turn: TurnId::new(1)
        }
    );
    let public = serde_json::to_value(&first.public).unwrap();
    assert!(public.get("tile_ids").is_none());
    assert!(public.get("exchange").is_none());
    assert!(
        public["players"]
            .as_array()
            .unwrap()
            .iter()
            .all(|player| player.get("rack").is_none())
    );
    let next = command(
        2,
        1,
        1,
        Command::Exchange {
            tile_ids: vec![second.own_rack.tiles[0].id],
        },
    );
    runtime.submit_command(2, 1, 1, &next).unwrap();
    runtime.advance_tick().unwrap();
    let expected = runtime.snapshot().unwrap();
    let log = ReplayLog::decode(&runtime.replay_log().unwrap().encode().unwrap()).unwrap();
    assert_eq!(
        verify_replay(simulation(), &log).unwrap().final_snapshot,
        expected
    );
    runtime.freeze_for_recovery();
    let restored =
        MatchRuntime::restore_from_recovery(simulation(), runtime.recovery_image().unwrap())
            .unwrap();
    assert_eq!(restored.snapshot().unwrap(), expected);
}
