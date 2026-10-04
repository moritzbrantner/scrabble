use scrabble_game::{
    commit::CommitError,
    dictionary::DictionaryError,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement},
    ruleset::{TileFace, english_fixture},
    session::{CommandError, GameSession},
    state::GameState,
};

fn envelope(sequence: u32, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(1),
        sequence,
        expected_turn: TurnId::new(0),
        command,
    }
}
fn game(revision: &str) -> (GameSession, Vec<Placement>) {
    for seed in 0..=255 {
        let mut rules = english_fixture();
        rules.dictionary.revision = revision.into();
        let mut session =
            GameSession::new(GameState::new(GameId::new(1), rules, [seed; 32]).unwrap());
        session.add_player(PlayerId::new(1), "Ada".into()).unwrap();
        session.add_player(PlayerId::new(2), "Lin".into()).unwrap();
        session
            .apply(PlayerId::new(1), 1, &envelope(1, Command::Start {}))
            .unwrap();
        let rack = session.state().rack(PlayerId::new(1)).unwrap();
        let letters: Option<Vec<_>> = ['A', 'T']
            .into_iter()
            .enumerate()
            .map(|(index, letter)| {
                rack.iter()
                    .find(|tile| tile.face() == (TileFace::Letter { letter }))
                    .map(|tile| Placement {
                        tile_id: tile.id(),
                        coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
                        blank_as: None,
                    })
            })
            .collect();
        if let Some(placements) = letters {
            return (session, placements);
        }
    }
    panic!("No deterministic AT fixture found");
}

#[test]
fn accepted_commit_scores_transfers_advances_and_clears_preview_once() {
    let (mut session, placements) = game("1");
    session
        .apply(
            PlayerId::new(1),
            2,
            &envelope(
                2,
                Command::Preview {
                    placements: placements.clone(),
                },
            ),
        )
        .unwrap();
    let command = envelope(3, Command::Commit { placements });
    session.apply(PlayerId::new(1), 3, &command).unwrap();
    let snapshot = session.player_snapshot(PlayerId::new(1)).unwrap();
    assert_eq!(snapshot.public.board.len(), 2);
    assert_eq!(snapshot.public.players[0].score, 4);
    assert_eq!(snapshot.public.remaining_tiles, 84);
    assert_eq!(snapshot.own_rack.tiles.len(), 7);
    session.state().verify_tile_conservation().unwrap();
    assert_eq!(
        snapshot.public.phase,
        Phase::Playing {
            active_player: PlayerId::new(2),
            turn: TurnId::new(1)
        }
    );
    assert!(snapshot.public.preview.is_none());
    let canonical = session.state().canonical_bytes().unwrap();
    assert_eq!(
        session.apply(PlayerId::new(1), 3, &command),
        Err(CommandError::StaleSequence)
    );
    let retry = envelope(4, command.command.clone());
    assert_eq!(
        session.apply(PlayerId::new(1), 4, &retry),
        Err(CommandError::StaleTurn)
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), canonical);
}

#[test]
fn dictionary_rejection_preserves_canonical_and_preview_and_allows_corrected_retry() {
    let (mut session, placements) = game("1");
    session
        .apply(
            PlayerId::new(1),
            2,
            &envelope(
                2,
                Command::Preview {
                    placements: placements.clone(),
                },
            ),
        )
        .unwrap();
    let before = session.state().canonical_bytes().unwrap();
    let preview = session.public_snapshot().preview;
    let mut invalid = placements.clone();
    invalid[0].coordinate = placements[1].coordinate;
    invalid[1].coordinate = placements[0].coordinate;
    assert_eq!(
        session.apply(
            PlayerId::new(1),
            3,
            &envelope(
                3,
                Command::Commit {
                    placements: invalid
                }
            )
        ),
        Err(CommandError::Commit(CommitError::Dictionary(
            DictionaryError::UnknownWord("TA".into())
        )))
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), before);
    assert_eq!(session.public_snapshot().preview, preview);
    session
        .apply(
            PlayerId::new(1),
            3,
            &envelope(3, Command::Commit { placements }),
        )
        .unwrap();
    assert_eq!(session.public_snapshot().players[0].score, 4);
}

#[test]
fn valid_main_word_is_rejected_when_a_cross_word_is_absent() {
    use scrabble_game::ruleset::TileDefinition;
    let (mut state, seed_placements, proposed) = (0..=255)
        .find_map(|seed| {
            let mut rules = english_fixture();
            rules.tiles = ['A', 'C', 'T']
                .into_iter()
                .map(|letter| TileDefinition {
                    face: TileFace::Letter { letter },
                    count: 34,
                    value: 1,
                })
                .collect();
            let mut state = GameState::new(GameId::new(1), rules, [seed; 32]).unwrap();
            state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
            state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
            state.deal_initial_racks().unwrap();
            let placements = |player, text: &str, row, column| -> Option<Vec<Placement>> {
                text.chars()
                    .enumerate()
                    .map(|(index, letter)| {
                        state
                            .rack(player)
                            .unwrap()
                            .iter()
                            .find(|tile| tile.face() == (TileFace::Letter { letter }))
                            .map(|tile| Placement {
                                tile_id: tile.id(),
                                coordinate: Coordinate::new(row, column + index as u8).unwrap(),
                                blank_as: None,
                            })
                    })
                    .collect()
            };
            let existing = placements(PlayerId::new(2), "AT", 7, 7)?;
            let proposed = placements(PlayerId::new(1), "CAT", 6, 6)?;
            Some((state, existing, proposed))
        })
        .expect("deterministic crossword fixture");
    state
        .place_tiles(PlayerId::new(2), &seed_placements)
        .unwrap();
    let before = state.canonical_bytes().unwrap();
    assert_eq!(
        scrabble_game::commit::apply(&mut state, PlayerId::new(1), &proposed),
        Err(CommitError::Dictionary(DictionaryError::UnknownWord(
            "AA".into()
        )))
    );
    assert_eq!(state.canonical_bytes().unwrap(), before);
    state.verify_tile_conservation().unwrap();
}

#[test]
fn uninstalled_dictionary_revision_cannot_silently_use_the_fixture() {
    let (mut session, placements) = game("2");
    let before = session.state().canonical_bytes().unwrap();
    assert_eq!(
        session.apply(
            PlayerId::new(1),
            2,
            &envelope(2, Command::Commit { placements })
        ),
        Err(CommandError::Commit(CommitError::Dictionary(
            DictionaryError::IdentityMismatch
        )))
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), before);
}

#[test]
fn replacement_draws_are_atomic_bounded_and_preserve_tile_conservation() {
    use scrabble_game::ruleset::TileDefinition;
    for (total, expected_draws) in [(4, 0), (5, 1), (6, 2)] {
        let (mut session, placements) = (0..=255)
            .find_map(|seed| {
                let mut rules = english_fixture();
                rules.identity.name = format!("replacement-bag-{total}");
                rules.maximum_players = 2;
                rules.rack_size = 2;
                rules.tiles = vec![
                    TileDefinition {
                        face: TileFace::Letter { letter: 'A' },
                        count: total - 2,
                        value: 1,
                    },
                    TileDefinition {
                        face: TileFace::Letter { letter: 'T' },
                        count: 2,
                        value: 1,
                    },
                ];
                let mut session =
                    GameSession::new(GameState::new(GameId::new(1), rules, [seed; 32]).unwrap());
                session.add_player(PlayerId::new(1), "Ada".into()).unwrap();
                session.add_player(PlayerId::new(2), "Lin".into()).unwrap();
                session
                    .apply(PlayerId::new(1), 1, &envelope(1, Command::Start {}))
                    .unwrap();
                let rack = session.state().rack(PlayerId::new(1)).unwrap();
                let placements: Option<Vec<_>> = ['A', 'T']
                    .into_iter()
                    .enumerate()
                    .map(|(index, letter)| {
                        rack.iter()
                            .find(|tile| tile.face() == (TileFace::Letter { letter }))
                            .map(|tile| Placement {
                                tile_id: tile.id(),
                                coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
                                blank_as: None,
                            })
                    })
                    .collect();
                placements.map(|placements| (session, placements))
            })
            .expect("deterministic small-bag opening");
        session.state().verify_tile_conservation().unwrap();
        let other_rack = session.player_snapshot(PlayerId::new(2)).unwrap().own_rack;
        let before: serde_json::Value =
            serde_json::from_slice(&session.state().canonical_bytes().unwrap()).unwrap();
        let bag = before["bag"].as_array().unwrap();
        let expected_ids: Vec<_> = bag
            .iter()
            .rev()
            .take(expected_draws)
            .map(|tile| tile["id"].as_str().unwrap().to_owned())
            .collect();
        let placed_ids: Vec<_> = placements.iter().map(|tile| tile.tile_id).collect();
        session
            .apply(
                PlayerId::new(1),
                2,
                &envelope(2, Command::Commit { placements }),
            )
            .unwrap();
        session.state().verify_tile_conservation().unwrap();
        let own = session.player_snapshot(PlayerId::new(1)).unwrap();
        let other = session.player_snapshot(PlayerId::new(2)).unwrap();
        assert_eq!(own.own_rack.tiles.len(), expected_draws);
        assert_eq!(
            own.own_rack
                .tiles
                .iter()
                .map(|tile| tile.id.get().to_string())
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert!(
            own.own_rack
                .tiles
                .iter()
                .all(|tile| !placed_ids.contains(&tile.id))
        );
        assert_eq!(other.own_rack, other_rack);
        assert_eq!(own.public, other.public);
        assert_eq!(own.public.remaining_tiles, 0);
        assert_eq!(own.public.players[0].rack_count, expected_draws as u8);
        assert_eq!(own.public.board.len(), 2);
        assert_eq!(own.public.players[0].score, 54);
        assert_eq!(
            own.public.phase,
            Phase::Playing {
                active_player: PlayerId::new(2),
                turn: TurnId::new(1)
            }
        );
    }
}

#[test]
fn non_active_owned_commit_cannot_change_the_playable_turn() {
    let (mut session, _) = game("1");
    let placements = session
        .state()
        .rack(PlayerId::new(2))
        .unwrap()
        .iter()
        .take(2)
        .enumerate()
        .map(|(index, tile)| Placement {
            tile_id: tile.id(),
            coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
            blank_as: match tile.face() {
                TileFace::Blank {} => Some('A'),
                TileFace::Letter { .. } => None,
            },
        })
        .collect();
    let command = CommandEnvelope {
        player_id: PlayerId::new(2),
        ..envelope(1, Command::Commit { placements })
    };
    let before = session.state().canonical_bytes().unwrap();
    assert_eq!(
        session.apply(PlayerId::new(2), 1, &command),
        Err(CommandError::NotActive)
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), before);
}
