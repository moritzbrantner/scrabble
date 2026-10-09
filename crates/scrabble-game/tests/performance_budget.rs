//! 50 ms native Scrabble operation targets; game construction is outside measurements.
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use scrabble_game::{
    dictionary::WordList,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement},
    ruleset::{ContentIdentity, TileFace, english_fixture},
    session::GameSession,
    state::GameState,
};

const BUDGET: Duration = Duration::from_millis(50);
const SAMPLES: usize = 32;

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

fn large_dictionary() -> Arc<WordList> {
    static WORDS: OnceLock<Arc<WordList>> = OnceLock::new();
    Arc::clone(WORDS.get_or_init(|| {
        // Construct an immutable, reproducible 50k-entry word set once outside
        // the measured operation. Its identity is intentionally test-specific.
        let mut text = String::from("AT\n");
        for value in 0..50_000_usize {
            let mut remainder = value;
            let mut chars = [b'A'; 4];
            for index in (0..4).rev() {
                chars[index] = b'A' + (remainder % 26) as u8;
                remainder /= 26;
            }
            text.push_str(std::str::from_utf8(&chars).unwrap());
            text.push('\n');
        }
        Arc::new(
            WordList::from_text(
                ContentIdentity {
                    name: "scrabble-authored-fixture".into(),
                    revision: "performance-50k".into(),
                },
                &text,
            )
            .expect("valid uppercase fixture dictionary"),
        )
    }))
}

fn prepared_word() -> (GameSession, CommandEnvelope) {
    // Seed search, dictionary creation, multiplayer admissions, and turn history
    // are setup; measured operations begin after this state is fully reachable.
    let mut rules = english_fixture();
    rules.dictionary.revision = "performance-50k".into();
    rules.scoreless_turn_limit = 40;
    let dictionary = large_dictionary();
    for seed in 0..=u8::MAX {
        let mut session = GameSession::with_dictionary(
            GameState::new(GameId::new(1), rules.clone(), [seed; 32])
                .expect("valid authored English ruleset"),
            dictionary.clone(),
        );
        for (player, name) in [(1, "Ada"), (2, "Lin"), (3, "Mia"), (4, "Noa")] {
            session.add_player(PlayerId::new(player), name.into()).unwrap();
        }
        session
            .apply(PlayerId::new(1), 1, &command(1, 1, 0, Command::Start {}))
            .expect("start playable game");

        let rack = session.state().rack(PlayerId::new(1)).unwrap();
        let placements: Option<Vec<Placement>> = ['A', 'T']
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

        if let Some(placements) = placements {
            // Two full four-player rotations produce realistic nonempty turn
            // history without changing the first legal opening word.
            for turn in 0..8_u64 {
                let player = turn % 4 + 1;
                let sequence = (turn / 4 + 1) as u32 + u32::from(player == 1);
                session
                    .apply(
                        PlayerId::new(player),
                        sequence,
                        &command(player, sequence, turn, Command::Pass {}),
                    )
                    .expect("valid sequential pass");
            }
            return (session, command(1, 4, 8, Command::Commit { placements }));
        }
    }
    panic!("The deterministic English fixture must admit an AT opening");
}

fn assert_p95_within_budget(label: &str, mut samples: Vec<Duration>) {
    assert_eq!(samples.len(), SAMPLES);
    samples.sort_unstable();
    let observed = samples[(SAMPLES * 95).div_ceil(100) - 1];
    eprintln!(
        "SCRABBLE_PERFORMANCE operation={label} p95_ms={:.3} budget_ms=50 samples={SAMPLES}",
        observed.as_secs_f64() * 1_000.0
    );
    // Shared GitHub runners may deschedule the process between two Instants.
    // Keep correctness blocking and timing visible; enforce time only on an
    // intentionally controlled reference runner.
    if std::env::var("SCRABBLE_PERF_ENFORCE").as_deref() == Ok("1") {
        assert!(
            observed <= BUDGET,
            "{label} p95 {:.3} ms exceeded the 50 ms native processing budget",
            observed.as_secs_f64() * 1_000.0
        );
    }
}

#[test]
fn committing_a_word_and_reading_the_public_board_fit_50ms() {
    let mut commits = Vec::with_capacity(SAMPLES);
    let mut projections = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let (mut session, placement) = prepared_word();

        let started = Instant::now();
        session
            .apply(PlayerId::new(1), 4, &placement)
            .expect("valid word should commit");
        commits.push(started.elapsed());

        let started = Instant::now();
        let snapshot = session.public_snapshot();
        projections.push(started.elapsed());

        assert_eq!(snapshot.board.len(), 2);
        assert_eq!(snapshot.players.len(), 4);
        assert_eq!(snapshot.history.len(), 9);
        assert_eq!(snapshot.players[0].score, 4);
        assert_eq!(
            snapshot.phase,
            Phase::Playing {
                active_player: PlayerId::new(2),
                turn: TurnId::new(9),
            }
        );
        assert!(snapshot.preview.is_none());
        session.state().verify_tile_conservation().unwrap();
    }

    assert_p95_within_budget("authoritative_word_commit", commits);
    assert_p95_within_budget("public_board_snapshot", projections);
}
