use scrabble_game::identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId};
use scrabble_game::protocol::{Command, CommandEnvelope, Phase, Placement};
use scrabble_game::ruleset::{TileFace, english_fixture};
use scrabble_game::session::{CommandError, GameSession};
use scrabble_game::state::GameState;

fn lobby() -> GameSession {
    let mut session =
        GameSession::new(GameState::new(GameId::new(1), english_fixture(), [7; 32]).unwrap());
    session.add_player(PlayerId::new(1), "Ada".into()).unwrap();
    session.add_player(PlayerId::new(2), "Lin".into()).unwrap();
    session
}
fn command(player: u64, sequence: u32, turn: u64, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(player),
        sequence,
        expected_turn: TurnId::new(turn),
        command,
    }
}
fn apply(session: &mut GameSession, envelope: &CommandEnvelope) -> Result<(), CommandError> {
    session.apply(envelope.player_id, envelope.sequence, envelope)
}
fn started() -> GameSession {
    let mut session = lobby();
    apply(&mut session, &command(1, 1, 0, Command::Start {})).unwrap();
    session
}
fn draft(session: &GameSession, column: u8) -> Command {
    let tile = &session.state().rack(PlayerId::new(1)).unwrap()[0];
    Command::Preview {
        placements: vec![Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(7, column).unwrap(),
            blank_as: match tile.face() {
                TileFace::Blank {} => Some('A'),
                TileFace::Letter { .. } => None,
            },
        }],
    }
}

#[test]
fn previews_replace_cancel_and_disconnect_without_touching_canonical_state() {
    let mut session = started();
    let bytes = session.state().canonical_bytes().unwrap();
    let rack = session.state().rack(PlayerId::new(1)).unwrap().to_vec();
    let first = draft(&session, 7);
    apply(&mut session, &command(1, 2, 0, first)).unwrap();
    assert_eq!(
        session.public_snapshot().preview.as_ref().unwrap().tiles[0]
            .coordinate
            .column(),
        7
    );
    let second = draft(&session, 8);
    apply(&mut session, &command(1, 3, 0, second)).unwrap();
    assert_eq!(
        session.public_snapshot().preview.as_ref().unwrap().tiles[0]
            .coordinate
            .column(),
        8
    );
    session.clear_preview_for(PlayerId::new(2));
    assert!(session.public_snapshot().preview.is_some());
    session.clear_preview_for(PlayerId::new(1));
    assert!(session.public_snapshot().preview.is_none());
    let again = draft(&session, 7);
    apply(&mut session, &command(1, 4, 0, again)).unwrap();
    apply(
        &mut session,
        &command(1, 5, 0, Command::Preview { placements: vec![] }),
    )
    .unwrap();
    assert!(session.public_snapshot().preview.is_none());
    assert!(session.public_snapshot().board.is_empty());
    assert_eq!(session.state().rack(PlayerId::new(1)).unwrap(), rack);
    assert_eq!(session.state().canonical_bytes().unwrap(), bytes);
}

#[test]
fn identity_sequence_and_turn_rejections_are_atomic_and_do_not_consume_sequence() {
    let mut session = started();
    let before = session.state().canonical_bytes().unwrap();
    for (envelope, error) in [
        (
            command(1, 1, 0, Command::Pass {}),
            CommandError::StaleSequence,
        ),
        (command(2, 1, 0, Command::Pass {}), CommandError::NotActive),
        (command(1, 2, 99, Command::Pass {}), CommandError::StaleTurn),
        (
            command(1, 2, 0, Command::Commit { placements: vec![] }),
            CommandError::Commit(scrabble_game::commit::CommitError::Placement(
                scrabble_game::placement::PlacementError::Empty,
            )),
        ),
    ] {
        assert_eq!(apply(&mut session, &envelope), Err(error));
        assert_eq!(session.state().canonical_bytes().unwrap(), before);
    }
    let mut envelope = command(1, 2, 0, Command::Pass {});
    assert_eq!(
        session.apply(PlayerId::new(2), 2, &envelope),
        Err(CommandError::WrongIdentity)
    );
    assert_eq!(
        session.apply(PlayerId::new(1), 3, &envelope),
        Err(CommandError::WrongSequence)
    );
    envelope.game_id = GameId::new(99);
    assert_eq!(apply(&mut session, &envelope), Err(CommandError::WrongGame));
    apply(&mut session, &command(1, 2, 0, Command::Pass {})).unwrap();
    let after = session.state().canonical_bytes().unwrap();
    assert_eq!(
        apply(&mut session, &command(1, 2, 0, Command::Pass {})),
        Err(CommandError::StaleSequence)
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), after);
}

#[test]
fn replay_and_turn_rollover_are_deterministic_and_clear_previews() {
    let mut first = started();
    let mut replay = started();
    let preview = draft(&first, 7);
    let commands = [
        command(1, 2, 0, preview),
        command(1, 3, 0, Command::Pass {}),
        command(2, 1, 1, Command::Pass {}),
        command(1, 4, 2, Command::Pass {}),
    ];
    for envelope in commands {
        apply(&mut first, &envelope).unwrap();
        apply(&mut replay, &envelope).unwrap();
        assert_eq!(
            first.state().canonical_bytes().unwrap(),
            replay.state().canonical_bytes().unwrap()
        );
        assert_eq!(first.public_snapshot(), replay.public_snapshot());
    }
    assert_eq!(
        first.state().phase(),
        &Phase::Playing {
            active_player: PlayerId::new(2),
            turn: TurnId::new(3)
        }
    );
    assert!(first.public_snapshot().preview.is_none());
    first.state().verify_tile_conservation().unwrap();
}

#[test]
fn only_the_host_can_start_and_invalid_preview_preserves_the_previous_one() {
    let mut session = lobby();
    assert_eq!(
        apply(&mut session, &command(2, 1, 0, Command::Start {})),
        Err(CommandError::NotHost)
    );
    apply(&mut session, &command(1, 1, 0, Command::Start {})).unwrap();
    let valid = draft(&session, 7);
    apply(&mut session, &command(1, 2, 0, valid)).unwrap();
    let before = session.public_snapshot();
    let Command::Preview { mut placements } = draft(&session, 8) else {
        unreachable!()
    };
    placements.push(placements[0].clone());
    assert!(
        apply(
            &mut session,
            &command(1, 3, 0, Command::Preview { placements })
        )
        .is_err()
    );
    assert_eq!(session.public_snapshot(), before);
    apply(
        &mut session,
        &command(1, 3, 0, Command::Preview { placements: vec![] }),
    )
    .unwrap();
}

#[test]
fn lobby_names_are_bounded_atomic_and_never_identity_authority() {
    let mut session = lobby();
    let rename = command(
        2,
        1,
        0,
        Command::SetName {
            display_name: "  Ada  ".into(),
        },
    );
    apply(&mut session, &rename).unwrap();
    let public = session.public_snapshot();
    assert_eq!(public.players[0].display_name, "Ada");
    assert_eq!(public.players[1].display_name, "Ada");
    assert_ne!(public.players[0].id, public.players[1].id);
    assert_eq!(
        apply(&mut session, &command(2, 2, 0, Command::Start {})),
        Err(CommandError::NotHost)
    );
    let before = session.state().canonical_bytes().unwrap();
    for name in [
        "".to_owned(),
        " ".to_owned(),
        "x".repeat(33),
        "bad\nname".to_owned(),
    ] {
        assert!(
            apply(
                &mut session,
                &command(2, 2, 0, Command::SetName { display_name: name })
            )
            .is_err()
        );
        assert_eq!(session.state().canonical_bytes().unwrap(), before);
    }
    let claim = command(
        1,
        2,
        0,
        Command::SetName {
            display_name: "Impostor".into(),
        },
    );
    assert_eq!(
        session.apply(PlayerId::new(2), 2, &claim),
        Err(CommandError::WrongIdentity)
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), before);
    apply(&mut session, &command(1, 1, 0, Command::Start {})).unwrap();
    let started = session.state().canonical_bytes().unwrap();
    assert!(
        apply(
            &mut session,
            &command(
                1,
                2,
                0,
                Command::SetName {
                    display_name: "Other".into()
                }
            )
        )
        .is_err()
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), started);
}
