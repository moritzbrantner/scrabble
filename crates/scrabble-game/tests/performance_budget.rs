//! Small 50 ms native operation budgets; no browser, network or fixture setup in samples.
use std::time::{Duration, Instant};

use scrabble_game::{
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement},
    ruleset::{TileFace, english_fixture},
    session::GameSession,
    state::GameState,
};

const BUDGET: Duration = Duration::from_millis(50);
const SAMPLES: usize = 32;

fn command(sequence: u32, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        version: ProtocolVersion,
        game_id: GameId::new(1),
        player_id: PlayerId::new(1),
        sequence,
        expected_turn: TurnId::new(0),
        command,
    }
}

fn prepared_word() -> (GameSession, CommandEnvelope) {
    // The seed search, dictionary and rack construction are explicitly outside
    // the authoritative move-acceptance measurement.
    for seed in 0..=u8::MAX {
        let mut session = GameSession::new(
            GameState::new(GameId::new(1), english_fixture(), [seed; 32])
                .expect("valid authored English ruleset"),
        );
        session.add_player(PlayerId::new(1), "Ada".into()).unwrap();
        session.add_player(PlayerId::new(2), "Lin".into()).unwrap();
        session
            .apply(PlayerId::new(1), 1, &command(1, Command::Start {}))
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
            return (session, command(2, Command::Commit { placements }));
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
    assert!(
        observed <= BUDGET,
        "{label} p95 {:.3} ms exceeded the 50 ms native processing budget",
        observed.as_secs_f64() * 1_000.0
    );
}

#[test]
fn committing_a_word_and_reading_the_public_board_fit_50ms() {
    let mut commits = Vec::with_capacity(SAMPLES);
    let mut projections = Vec::with_capacity(SAMPLES);

    for _ in 0..SAMPLES {
        let (mut session, placement) = prepared_word();

        let started = Instant::now();
        session
            .apply(PlayerId::new(1), 2, &placement)
            .expect("valid word should commit");
        commits.push(started.elapsed());

        let started = Instant::now();
        let snapshot = session.public_snapshot();
        projections.push(started.elapsed());

        // Semantic acceptance is independent of timing.
        assert_eq!(snapshot.board.len(), 2);
        assert_eq!(snapshot.players[0].score, 4);
        assert_eq!(
            snapshot.phase,
            Phase::Playing {
                active_player: PlayerId::new(2),
                turn: TurnId::new(1),
            }
        );
        assert!(snapshot.preview.is_none());
        session.state().verify_tile_conservation().unwrap();
    }

    assert_p95_within_budget("authoritative_word_commit", commits);
    assert_p95_within_budget("public_board_snapshot", projections);
}
