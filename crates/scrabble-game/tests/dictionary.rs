use scrabble_game::{
    commit::{self, CommitError},
    dictionary::{Dictionary, DictionaryError, WordList, WordListError},
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    placement::PlacementError,
    protocol::{Command, CommandEnvelope, Placement},
    ruleset::{ContentIdentity, TileFace, english_fixture},
    session::{CommandError, GameSession},
    state::GameState,
};
use std::sync::{Arc, Mutex};

fn identity() -> ContentIdentity {
    ContentIdentity {
        name: "test-owned".into(),
        revision: "1".into(),
    }
}
fn opening() -> (GameState, Vec<Placement>) {
    (0..=255)
        .find_map(|seed| {
            let mut rules = english_fixture();
            rules.dictionary = identity();
            let mut state = GameState::new(GameId::new(1), rules, [seed; 32]).unwrap();
            state.add_player(PlayerId::new(1), "Ada".into()).unwrap();
            state.add_player(PlayerId::new(2), "Lin".into()).unwrap();
            state.deal_initial_racks().unwrap();
            let placements: Option<Vec<_>> = ['A', 'T']
                .into_iter()
                .enumerate()
                .map(|(index, letter)| {
                    state
                        .rack(PlayerId::new(1))
                        .unwrap()
                        .iter()
                        .find(|tile| tile.face() == (TileFace::Letter { letter }))
                        .map(|tile| Placement {
                            tile_id: tile.id(),
                            coordinate: Coordinate::new(7, 7 + index as u8).unwrap(),
                            blank_as: None,
                        })
                })
                .collect();
            placements.map(|placements| (state, placements))
        })
        .expect("deterministic AT rack")
}
struct RecordingDictionary {
    list: WordList,
    calls: Mutex<Vec<String>>,
}
impl Dictionary for RecordingDictionary {
    fn identity(&self) -> &ContentIdentity {
        self.list.identity()
    }
    fn contains(&self, word: &str) -> bool {
        self.calls.lock().unwrap().push(word.to_owned());
        self.list.contains(word)
    }
}

#[test]
fn configured_dictionary_controls_acceptance_without_changing_storage_semantics() {
    let (state, placements) = opening();
    let rejected = Arc::new(WordList::from_text(identity(), "TA\n").unwrap());
    let before = state.canonical_bytes().unwrap();
    let mut session = GameSession::with_dictionary(state.clone(), rejected);
    let envelope = CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(1),
        sequence: 1,
        expected_turn: TurnId::new(0),
        command: Command::Commit {
            placements: placements.clone(),
        },
    };
    assert_eq!(
        session.apply(PlayerId::new(1), 1, &envelope),
        Err(CommandError::Commit(CommitError::Dictionary(
            DictionaryError::InvalidWords {
                words: vec!["AT".into()]
            }
        )))
    );
    assert_eq!(session.state().canonical_bytes().unwrap(), before);
    let mut first = GameSession::with_dictionary(
        state.clone(),
        Arc::new(WordList::from_text(identity(), "TA\nAT\n").unwrap()),
    );
    let mut reversed = GameSession::with_dictionary(
        state,
        Arc::new(WordList::from_text(identity(), "AT\nTA\nAT\n").unwrap()),
    );
    first.apply(PlayerId::new(1), 1, &envelope).unwrap();
    reversed.apply(PlayerId::new(1), 1, &envelope).unwrap();
    assert_eq!(
        first.state().canonical_bytes().unwrap(),
        reversed.state().canonical_bytes().unwrap()
    );
    assert_eq!(first.public_snapshot().dictionary, identity());
    assert_eq!(first.public_snapshot().players[0].score, 4);
    first.state().verify_tile_conservation().unwrap();
}

#[test]
fn structural_rejection_and_identity_mismatch_do_not_query_dictionary() {
    let (mut state, placements) = opening();
    let dictionary = RecordingDictionary {
        list: WordList::from_text(identity(), "AT\n").unwrap(),
        calls: Mutex::new(vec![]),
    };
    let before = state.canonical_bytes().unwrap();
    assert_eq!(
        commit::apply_with_dictionary(&mut state, PlayerId::new(1), &[], &dictionary),
        Err(CommitError::Placement(PlacementError::Empty))
    );
    assert!(dictionary.calls.lock().unwrap().is_empty());
    let mut wrong = identity();
    wrong.revision = "2".into();
    let wrong = RecordingDictionary {
        list: WordList::from_text(wrong, "AT\n").unwrap(),
        calls: Mutex::new(vec![]),
    };
    assert_eq!(
        commit::apply_with_dictionary(&mut state, PlayerId::new(1), &placements, &wrong),
        Err(CommitError::Dictionary(DictionaryError::IdentityMismatch))
    );
    assert!(wrong.calls.lock().unwrap().is_empty());
    assert_eq!(state.canonical_bytes().unwrap(), before);
    commit::apply_with_dictionary(&mut state, PlayerId::new(1), &placements, &dictionary).unwrap();
    assert_eq!(*dictionary.calls.lock().unwrap(), ["AT"]);
}

#[test]
fn word_lists_are_exact_local_sets_and_feedback_has_a_structured_contract() {
    let list = WordList::from_text(identity(), "TA\r\nAT\r\n\r\nAT\nÄR\n").unwrap();
    assert_eq!(list.words().collect::<Vec<_>>(), ["AT", "TA", "ÄR"]);
    assert!(list.contains("AT"));
    assert!(!list.contains("at"));
    for text in ["at\n", " AT\n", "AT \n", "AT2\n", "A-T\n"] {
        assert!(matches!(
            WordList::from_text(identity(), text),
            Err(WordListError::InvalidWord { line: 1 })
        ));
    }
    assert!(matches!(
        WordList::from_text(identity(), "\n"),
        Err(WordListError::Empty)
    ));
    assert!(matches!(
        WordList::from_text(
            ContentIdentity {
                name: "".into(),
                revision: "1".into()
            },
            "AT\n"
        ),
        Err(WordListError::InvalidIdentity)
    ));
    let feedback = DictionaryError::InvalidWords {
        words: vec!["AA".into(), "TT".into()],
    };
    let value = serde_json::to_value(&feedback).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "kind": "invalid_words", "words": ["AA", "TT"] })
    );
    assert_eq!(
        serde_json::from_value::<DictionaryError>(value).unwrap(),
        feedback
    );
}
