//! Retrospective #96 capacity regression. Backend856db9b already existed when
//! the legal overflow was found; compact production repair3631a08 preceded this
//! durable test. This is not a claim of test-before-fix chronology.
//! The authored corpus/trace are fixed inputs; history, board and racks come only
//! from real native admission, deterministic deals and accepted commands.
use game_server::protocol::{
    MAX_SNAPSHOT_PAYLOAD_BYTES, SnapshotFrame, encode_snapshot, snapshot_hash,
};
use scrabble_game::{
    dictionary::WordList,
    identity::{Coordinate, GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Placement, TurnAction},
    ruleset::{
        BoardShape, ContentIdentity, Premium, PremiumPolicy, TileDefinition, TileFace,
        WordLengthMetric,
    },
    session::GameSession,
    state::GameState,
};
use scrabble_server::profiles::{
    BoardProfile, MatchRules, TileProfile, WordListProfile, compose_profiles,
};

fn identity(name: &str) -> ContentIdentity {
    ContentIdentity {
        name: name.into(),
        revision: "1".into(),
    }
}
const TRACE: &[&[(u8, u8)]] = &[
    &[(3, 7), (4, 7), (5, 7), (6, 7), (7, 7), (8, 7), (9, 7)],
    &[(3, 6), (4, 6), (5, 6), (6, 6), (7, 6), (8, 6), (9, 6)],
    &[
        (2, 8),
        (3, 8),
        (4, 8),
        (5, 8),
        (6, 8),
        (7, 8),
        (8, 8),
        (9, 8),
    ],
    &[(3, 9), (4, 9), (5, 9), (6, 9), (7, 9), (8, 9), (9, 9)],
    &[(3, 5), (4, 5), (5, 5), (6, 5), (7, 5), (8, 5), (9, 5)],
    &[
        (2, 10),
        (3, 10),
        (4, 10),
        (5, 10),
        (6, 10),
        (7, 10),
        (8, 10),
        (9, 10),
    ],
    &[
        (1, 11),
        (2, 11),
        (3, 11),
        (4, 11),
        (5, 11),
        (6, 11),
        (7, 11),
        (8, 11),
        (9, 11),
    ],
    &[
        (1, 12),
        (2, 12),
        (3, 12),
        (4, 12),
        (5, 12),
        (6, 12),
        (7, 12),
        (8, 12),
    ],
    &[
        (0, 13),
        (1, 13),
        (2, 13),
        (3, 13),
        (4, 13),
        (5, 13),
        (6, 13),
        (7, 13),
        (8, 13),
    ],
    &[(4, 4), (5, 4), (6, 4), (7, 4), (8, 4), (9, 4), (10, 4)],
    &[
        (0, 14),
        (1, 14),
        (2, 14),
        (3, 14),
        (4, 14),
        (5, 14),
        (6, 14),
        (7, 14),
    ],
    &[
        (10, 5),
        (10, 6),
        (10, 7),
        (10, 8),
        (10, 9),
        (10, 10),
        (10, 11),
    ],
    &[
        (11, 4),
        (11, 5),
        (11, 6),
        (11, 7),
        (11, 8),
        (11, 9),
        (11, 10),
    ],
    &[
        (4, 3),
        (5, 3),
        (6, 3),
        (7, 3),
        (8, 3),
        (9, 3),
        (10, 3),
        (11, 3),
        (12, 3),
    ],
    &[
        (5, 2),
        (6, 2),
        (7, 2),
        (8, 2),
        (9, 2),
        (10, 2),
        (11, 2),
        (12, 2),
        (13, 2),
        (14, 2),
    ],
    &[
        (6, 1),
        (7, 1),
        (8, 1),
        (9, 1),
        (10, 1),
        (11, 1),
        (12, 1),
        (13, 1),
    ],
    &[(7, 0), (8, 0), (9, 0), (10, 0), (11, 0), (12, 0), (13, 0)],
    &[(12, 4), (12, 5), (12, 6), (12, 7), (12, 8), (12, 9)],
    &[(13, 3), (13, 4), (13, 5), (13, 6), (13, 7), (13, 8)],
    &[(14, 0), (14, 1), (14, 3), (14, 4), (14, 5), (14, 6)],
    &[(2, 5), (2, 6), (2, 7), (2, 9)],
    &[(1, 6), (1, 7), (1, 8), (1, 9), (1, 10)],
    &[(0, 8), (0, 9), (0, 10), (0, 11), (0, 12)],
    &[(9, 12), (14, 7)],
];
fn command(session: &mut GameSession, actor: u64, sequence: u32, turn: u64, command: Command) {
    session
        .apply(
            PlayerId::new(actor),
            sequence,
            &CommandEnvelope {
                version: ProtocolVersion,
                game_id: GameId::new(1),
                player_id: PlayerId::new(actor),
                sequence,
                expected_turn: TurnId::new(turn),
                command,
            },
        )
        .unwrap();
}
fn assert_wire_capacity(session: &GameSession, turn: usize, stage: &str) -> usize {
    let mut largest = 0;
    for actor in 1..=4 {
        let snapshot = session.player_snapshot(PlayerId::new(actor)).unwrap();
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        largest = largest.max(bytes.len());
        assert!(
            bytes.len() <= MAX_SNAPSHOT_PAYLOAD_BYTES,
            "turn {turn} {stage}, recipient {actor}: {}-byte real player projection exceeds {MAX_SNAPSHOT_PAYLOAD_BYTES}",
            bytes.len()
        );
        let frame = SnapshotFrame {
            tick: turn as u64,
            state_hash: snapshot_hash(turn as u64, &bytes),
            payload: bytes,
        };
        assert!(
            encode_snapshot(&frame).is_ok(),
            "real player projection must encode on the native wire"
        );
    }
    largest
}
#[test]
fn legal_composed_hex_history_with_analysis_fits_every_player_wire_projection() {
    // Four-byte alphabetic uppercase scalars, all native-supported single-letter
    // faces. Counts11+63*3=200 are the existing maximum supported tile total.
    let tiles = TileProfile::new(
        identity("hex-payload-tiles"),
        (0x10400..=0x10427)
            .chain(0x104b0..=0x104d3)
            .filter_map(char::from_u32)
            .take(64)
            .enumerate()
            .map(|(index, letter)| TileDefinition {
                face: TileFace::Letter { letter },
                count: if index == 0 { 11 } else { 3 },
                value: u16::MAX,
            })
            .collect(),
    )
    .unwrap();
    let words = WordListProfile::new(
        WordList::from_text(
            identity("hex-payload-words"),
            include_str!("support/hex_payload_words.txt"),
        )
        .unwrap(),
    )
    .unwrap();
    let board = BoardProfile::with_geometry(
        identity("hex-payload-board"),
        15,
        vec![Premium::Normal; 169],
        BoardShape::Hexagon,
        PremiumPolicy::AllWord,
        WordLengthMetric::Tiles,
    )
    .unwrap();
    let composed = compose_profiles(
        &words,
        &tiles,
        &board,
        MatchRules {
            rack_size: 15,
            bingo_bonus: 0,
            exchange_minimum_bag: 1,
            scoreless_turn_limit: u16::MAX,
            minimum_players: 2,
            maximum_players: 4,
        },
    )
    .unwrap();
    // These are actual composition fingerprints, not artificial192-byte revisions.
    assert_eq!(composed.rules.identity.name, "composed-profiles");
    assert_eq!(
        composed.rules.identity.revision.len(),
        "1+composition-sha256:".len() + 64
    );
    assert_eq!(
        composed.rules.dictionary.revision.len(),
        "1+word-profile-sha256:".len() + 64
    );
    let mut session = GameSession::with_dictionary(
        GameState::new(GameId::new(1), composed.rules, [7; 32]).unwrap(),
        composed.dictionary,
    );
    for actor in 1..=4 {
        session
            .add_player(PlayerId::new(actor), "𐐀".repeat(32))
            .unwrap();
    }
    command(&mut session, 1, 1, 0, Command::Start {});
    let mut sequences = [1, 0, 0, 0];
    let mut largest = 0;
    for (index, coordinates) in TRACE.iter().enumerate() {
        let actor = 1 + (index % 4) as u64;
        let placements: Vec<_> = coordinates
            .iter()
            .zip(session.state().rack(PlayerId::new(actor)).unwrap())
            .map(|(&(row, column), tile)| Placement {
                tile_id: tile.id(),
                coordinate: Coordinate::new(row, column).unwrap(),
                blank_as: None,
            })
            .collect();
        assert_eq!(
            placements.len(),
            coordinates.len(),
            "each actual rack must fund its move"
        );
        sequences[index % 4] += 1;
        command(
            &mut session,
            actor,
            sequences[index % 4],
            index as u64,
            Command::Preview {
                placements: placements.clone(),
            },
        );
        largest = largest.max(assert_wire_capacity(&session, index + 1, "preview"));
        sequences[index % 4] += 1;
        command(
            &mut session,
            actor,
            sequences[index % 4],
            index as u64,
            Command::Commit { placements },
        );
        largest = largest.max(assert_wire_capacity(&session, index + 1, "accepted"));
    }
    let snapshot = session.public_snapshot();
    assert_eq!(snapshot.board.len(), 167);
    assert_eq!(snapshot.history.len(), 24);
    assert!(snapshot.history.iter().all(|turn| turn.analysis.is_some()));
    assert_eq!(
        snapshot
            .history
            .iter()
            .map(|turn| match &turn.action {
                TurnAction::Commit { words, .. } => words.len(),
                _ => panic!("trace contains only accepted commits"),
            })
            .sum::<usize>(),
        325
    );
    eprintln!(
        "legal composed hex maximum real player projection: {largest} bytes; native limit: {MAX_SNAPSHOT_PAYLOAD_BYTES}"
    );
    // No u16 assertion on private canonical data: PlayerScoped publication does
    // not frame it as a public snapshot. Native recovery has its own256MiB limit.
}
