//! Real Scrabble application for browser creation acceptance; no replacement game/session rules.
#[path = "browser_game_fixture/hexagonal.rs"]
mod hexagonal;
#[path = "../tests/support/profile_catalog.rs"]
mod profile_catalog;

use game_server::GameSimulation;
use scrabble_game::{
    identity::{PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, PlayerSnapshot},
    ruleset::TileFace,
};
use scrabble_server::{
    config::ServerConfig,
    factory::{self, MatchFactory},
    hosting,
};
use std::{
    collections::BTreeMap,
    error::Error,
    path::{Path, PathBuf},
};
use tokio::sync::mpsc;
use wtransport::Identity;

/// Select real creation inputs whose native deal supports the authored AT opening.
/// No rack, seed key, or alternate gameplay implementation enters browser metadata.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Opening {
    request_id: String,
    requested_at: u64,
}
fn opening(
    factory: &MatchFactory,
    consecutive: bool,
    excluded_request: Option<&str>,
) -> Result<Opening, Box<dyn Error + Send + Sync>> {
    let requested_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    for nonce in 0..10_000_u32 {
        let request_id = format!("{nonce:032x}");
        if excluded_request == Some(request_id.as_str()) {
            continue;
        }
        let id = factory.board_creation_id(&request_id, requested_at)?;
        let mut simulation = factory.create(&id)?;
        for player in 1..=3 {
            simulation.add_player(player)?;
        }
        for (player, sequence, command) in [
            (
                1,
                1,
                Command::ClaimBoard {
                    request_id: request_id.clone(),
                },
            ),
            (
                2,
                1,
                Command::SetName {
                    display_name: "Ada".into(),
                },
            ),
            (
                3,
                1,
                Command::SetName {
                    display_name: "Lin".into(),
                },
            ),
            (1, 2, Command::Start {}),
        ] {
            let payload = serde_json::to_vec(&CommandEnvelope {
                version: ProtocolVersion,
                game_id: factory::game_id(&id),
                player_id: PlayerId::new(u64::from(player)),
                sequence,
                expected_turn: TurnId::new(0),
                command,
            })?;
            simulation.apply_command(player, sequence, &payload)?;
        }
        let own: PlayerSnapshot = serde_json::from_slice(&simulation.snapshot_for(2)?.payload)?;
        let next: PlayerSnapshot = serde_json::from_slice(&simulation.snapshot_for(3)?.payload)?;
        let next_can_extend = !consecutive
            || next
                .own_rack
                .tiles
                .iter()
                .any(|tile| tile.face == (TileFace::Letter { letter: 'C' }));
        if ['A', 'T'].into_iter().all(|letter| {
            own.own_rack
                .tiles
                .iter()
                .any(|tile| tile.face == (TileFace::Letter { letter }))
        }) && next_can_extend
        {
            return Ok(Opening {
                request_id,
                requested_at,
            });
        }
    }
    Err("could not select a native authored-dictionary opening".into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    if std::env::args().nth(1).as_deref() == Some("--fresh-opening") {
        let directory = PathBuf::from(std::env::args().nth(2).ok_or("missing fixture directory")?);
        let values = serde_json::from_slice::<BTreeMap<String, String>>(&std::fs::read(
            directory.join("fixture-config.json"),
        )?)?;
        let config = ServerConfig::from_values(&values)?;
        let factory = MatchFactory::new(&config)?;
        let excluded = std::env::args().nth(3);
        let opening = opening(&factory, config.dictionary.is_some(), excluded.as_deref())?;
        println!("{}", serde_json::to_string(&opening)?);
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--hex-replay") {
        let directory = PathBuf::from(std::env::args().nth(2).ok_or("missing fixture directory")?);
        let id = std::env::args().nth(3).ok_or("missing match identity")?;
        println!(
            "{}",
            serde_json::to_string(&hexagonal::replay(&directory, &id)?)?
        );
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--hex-opening") {
        let directory = PathBuf::from(std::env::args().nth(2).ok_or("missing fixture directory")?);
        println!(
            "{}",
            serde_json::to_string(&hexagonal::opening(&directory)?)?
        );
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--multi-letter-opening") {
        let directory = PathBuf::from(std::env::args().nth(2).ok_or("missing fixture directory")?);
        let excluded = std::env::args().nth(3);
        let opening = multi_letter::opening(&directory, excluded.as_deref())?;
        println!("{}", serde_json::to_string(&opening)?);
        return Ok(());
    }
    let directory = PathBuf::from(std::env::args().nth(1).ok_or("missing fixture directory")?);
    let origin = std::env::args().nth(2).ok_or("missing board origin")?;
    std::fs::create_dir(&directory)?;
    let identity = Identity::self_signed(["localhost", "127.0.0.1"])?;
    let certificate_pem = directory.join("cert.pem");
    let key_pem = directory.join("key.pem");
    identity
        .certificate_chain()
        .store_pemfile(&certificate_pem)
        .await?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&key_pem)?);
    identity
        .private_key()
        .store_secret_pemfile(&key_pem)
        .await?;
    let udp = std::net::UdpSocket::bind("127.0.0.1:0")?;
    let port = udp.local_addr()?.port();
    let status = std::net::TcpListener::bind("127.0.0.1:0")?;
    let status_port = status.local_addr()?.port();
    let api = std::net::TcpListener::bind("127.0.0.1:0")?;
    let api_port = api.local_addr()?.port();
    drop((udp, status, api));
    let mut values = BTreeMap::from([
        ("SCRABBLE_PORT".into(), port.to_string()),
        ("SCRABBLE_STATUS_PORT".into(), status_port.to_string()),
        ("SCRABBLE_API_PORT".into(), api_port.to_string()),
        ("SCRABBLE_BOARD_ORIGIN".into(), origin),
        ("SCRABBLE_MATCH_IDS".into(), String::new()),
        // One shared fixture server hosts every match the browser suites create; the
        // production default (16) is too small for the whole suite, so use the bound (64).
        ("SCRABBLE_MAX_MATCHES".into(), "64".into()),
        (
            "SCRABBLE_CERT_PEM".into(),
            certificate_pem.to_str().ok_or("invalid cert path")?.into(),
        ),
        (
            "SCRABBLE_KEY_PEM".into(),
            key_pem.to_str().ok_or("invalid key path")?.into(),
        ),
        (
            "SCRABBLE_SEED_FILE".into(),
            directory
                .join("seed")
                .to_str()
                .ok_or("invalid seed path")?
                .into(),
        ),
        (
            "SCRABBLE_RECOVERY_DIR".into(),
            directory
                .join("recovery")
                .to_str()
                .ok_or("invalid recovery path")?
                .into(),
        ),
    ]);
    let configured = std::env::args().nth(3).as_deref() == Some("playable");
    if configured {
        let word_file = directory.join("words.txt");
        let words = "TA\nCTA\n";
        std::fs::write(&word_file, words)?;
        values.insert(
            "SCRABBLE_DICTIONARY_FILE".into(),
            word_file.to_str().ok_or("invalid word-list path")?.into(),
        );
        values.insert(
            "SCRABBLE_DICTIONARY_NAME".into(),
            "scrabble-browser-custom".into(),
        );
        values.insert("SCRABBLE_DICTIONARY_REVISION".into(), "1".into());
    }
    // Only the real browser fixture advertises these authored catalog sources. Its exact
    // deployment word set, tiles, premiums and rule defaults remain legacy-equivalent.
    let catalog = profile_catalog::write_catalog(
        &directory,
        if configured {
            "TA\nCTA\n"
        } else {
            profile_catalog::LEGACY_WORDS
        },
    )?;
    // A separate acceptance invocation exercises an untouched, non-legacy default
    // through the real catalog authority; ordinary fixtures retain their exact defaults.
    if std::env::var("SCRABBLE_BROWSER_COMPACT_DEFAULT").as_deref() == Ok("1") {
        let mut document: serde_json::Value = serde_json::from_slice(&std::fs::read(&catalog)?)?;
        document["defaultSelection"]["board"] = serde_json::json!("acceptance-compact-board");
        std::fs::write(&catalog, serde_json::to_vec_pretty(&document)?)?;
    }
    if std::env::var("SCRABBLE_BROWSER_EXCHANGE_REJECTION").as_deref() == Ok("1") {
        let mut document: serde_json::Value = serde_json::from_slice(&std::fs::read(&catalog)?)?;
        let mut tiles = document["tileProfiles"][0]["tiles"].clone();
        for tile in tiles.as_array_mut().ok_or("missing authored tiles")? {
            tile["count"] = serde_json::json!(1);
        }
        document["tileProfiles"]
            .as_array_mut()
            .ok_or("missing tile profiles")?
            .push(serde_json::json!({
                "identity":{"name":"acceptance-low-bag-tiles","revision":"1"},
                "label":"Low-bag test tiles","tiles":tiles
            }));
        std::fs::write(&catalog, serde_json::to_vec_pretty(&document)?)?;
    }
    // The browser understands multi-letter faces (#98), so every fixture run offers them.
    multi_letter::add_sources(&directory, &catalog)?;
    hexagonal::add_sources(&directory, &catalog)?;
    values.insert(
        "SCRABBLE_PROFILE_CATALOG_FILE".into(),
        catalog.to_str().ok_or("invalid catalog path")?.into(),
    );
    values.insert(
        "SCRABBLE_MATCH_CONFIGURATION_DIR".into(),
        directory
            .join("configurations")
            .to_str()
            .ok_or("invalid configuration path")?
            .into(),
    );
    retain_legacy_backend_if_catalog_is_unavailable(&mut values)?;
    // Test-only paths and configuration let later tests select fresh real creation inputs.
    // The persisted derivation key remains in its private seed file and never reaches metadata.
    std::fs::write(
        directory.join("fixture-config.json"),
        serde_json::to_vec(&values)?,
    )?;
    let config = ServerConfig::from_values(&values)?;
    let factory = MatchFactory::new(&config)?;
    let opening = opening(&factory, configured, None)?;
    let reconnect_opening = self::opening(&factory, configured, Some(&opening.request_id))?;
    let metadata = serde_json::json!({ "endpoint": format!("https://127.0.0.1:{port}/game"),
        "api": format!("http://127.0.0.1:{api_port}"), "status": format!("http://127.0.0.1:{status_port}"),
        "certificateHash": identity.certificate_chain().as_slice()[0].hash().as_ref(),
        "opening": opening, "reconnectOpening": reconnect_opening });
    std::fs::write(
        directory.join("connection.json"),
        serde_json::to_vec(&metadata)?,
    )?;
    let (sender, receiver) = mpsc::channel(1);
    let signal = tokio::spawn(async move {
        tokio::signal::ctrl_c().await?;
        sender.send(()).await.map_err(std::io::Error::other)
    });
    let result = hosting::serve(config, receiver).await;
    signal.abort();
    let _ = signal.await;
    result
}

/// Keep pre-feature executable fixtures runnable without pretending the new catalog works.
/// The new acceptance still requires GET /profiles = 200 and the actual creation controls.
/// Only the specifically absent #94 keys permit this baseline compatibility branch.
fn retain_legacy_backend_if_catalog_is_unavailable(
    values: &mut std::collections::BTreeMap<String, String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Err(error) = scrabble_server::config::ServerConfig::from_values(values) {
        let message = error.to_string();
        if message == "unknown server configuration key SCRABBLE_PROFILE_CATALOG_FILE"
            || message == "unknown server configuration key SCRABBLE_MATCH_CONFIGURATION_DIR"
        {
            values.remove("SCRABBLE_PROFILE_CATALOG_FILE");
            values.remove("SCRABBLE_MATCH_CONFIGURATION_DIR");
            scrabble_server::config::ServerConfig::from_values(values)?;
        } else {
            return Err(error.into());
        }
    }
    Ok(())
}

/// #98 acceptance harness: an authored multi-letter tile profile and a compatible word list,
/// selected through the real catalog, plus a native search for a v3 opening deal.
///
/// Every fixture run offers these catalog entries, so the default `bun run test:browser`
/// covers `multi-letter-tiles.spec.ts`; see docs/specs/multi-letter-browser.md.
mod multi_letter {
    use super::{
        Command, CommandEnvelope, Error, GameSimulation, Opening, Path, ProtocolVersion, TurnId,
    };
    use game_server::MatchId;
    use ring::hmac;
    use scrabble_game::{
        dictionary::WordList,
        identity::PlayerId,
        ruleset::{ContentIdentity, Premium, TileDefinition, TileFace},
        state::GameState,
    };
    use scrabble_server::{
        config::ServerConfig,
        factory::{self, MatchFactory},
        profiles::{self, BoardProfile, MatchRules, TileProfile, WordListProfile},
    };
    use std::collections::BTreeMap;

    type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
    pub const TILES: &str = "acceptance-multi-letter-tiles";
    pub const WORDS: &str = "acceptance-multi-letter-words";
    const BOARD: &str = "acceptance-classic-board";

    /// 29 tiles: single letters that share letters with CH and SCH, two blanks.
    pub(super) fn tiles() -> serde_json::Value {
        let mut tiles: Vec<_> = [
            ("A", 6, 1),
            ("E", 4, 1),
            ("N", 2, 1),
            ("S", 2, 1),
            ("T", 4, 1),
            ("C", 1, 3),
            ("H", 1, 2),
            ("CH", 4, 5),
            ("SCH", 3, 8),
        ]
        .into_iter()
        .map(|(face, count, value)| {
            let face = if face.chars().count() == 1 {
                serde_json::json!({"kind":"letter","letter":face})
            } else {
                serde_json::json!({"kind":"letters","letters":face})
            };
            serde_json::json!({"face":face,"count":count,"value":value})
        })
        .collect();
        tiles.push(serde_json::json!({"face":{"kind":"blank"},"count":2,"value":0}));
        serde_json::Value::Array(tiles)
    }

    pub fn add_sources(directory: &Path, catalog: &Path) -> Result<()> {
        std::fs::write(directory.join("profile-multi-letter.txt"), "CHAT\nSCHACH\n")?;
        let mut document: serde_json::Value = serde_json::from_slice(&std::fs::read(catalog)?)?;
        document["wordLists"]
            .as_array_mut()
            .ok_or("missing word lists")?
            .push(serde_json::json!({
                "identity":{"name":WORDS,"revision":"1"},
                "label":"Multi-letter test words","file":"profile-multi-letter.txt"
            }));
        document["tileProfiles"]
            .as_array_mut()
            .ok_or("missing tile profiles")?
            .push(serde_json::json!({
                "identity":{"name":TILES,"revision":"1"},
                "label":"Multi-letter test tiles (CH, SCH)","tiles":tiles()
            }));
        std::fs::write(catalog, serde_json::to_vec_pretty(&document)?)?;
        Ok(())
    }

    fn row<'a>(
        document: &'a serde_json::Value,
        kind: &str,
        name: &str,
    ) -> Result<&'a serde_json::Value> {
        document[kind]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["identity"]["name"] == name))
            .ok_or_else(|| format!("catalog lacks {kind} {name}").into())
    }

    fn has(rack: &[scrabble_game::state::Tile], face: &TileFace) -> bool {
        rack.iter().any(|tile| &tile.face() == face)
    }

    /// Deal racks for seats 2 and 3 exactly as the board-started native match does.
    pub(super) fn deal(
        id: &MatchId,
        rules: &scrabble_game::ruleset::Ruleset,
        key: &hmac::Key,
    ) -> Result<GameState> {
        let mut derivation = hmac::Context::with_key(key);
        derivation.update(b"scrabble/deal/v1\0");
        derivation.update(id.as_str().as_bytes());
        let seed: [u8; 32] = derivation.sign().as_ref().try_into()?;
        let mut state = GameState::new(factory::game_id(id), rules.clone(), seed)?;
        state.add_player(PlayerId::new(2), "Ada".into())?;
        state.add_player(PlayerId::new(3), "Lin".into())?;
        state.deal_initial_racks()?;
        Ok(state)
    }

    pub(super) fn creation_id(
        key: &hmac::Key,
        domain: &[u8],
        prefix: &str,
        request_id: &str,
        at: u64,
    ) -> Result<MatchId> {
        let mut derivation = hmac::Context::with_key(key);
        derivation.update(domain);
        derivation.update(&at.to_be_bytes());
        derivation.update(request_id.as_bytes());
        let tag = derivation.sign();
        let suffix: String = tag.as_ref()[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(MatchId::new(format!("{prefix}_{at:016x}_{suffix}"))?)
    }

    /// Find a version-3 creation whose real deal gives Ada CH, A, T and a blank and Lin SCH
    /// and A. The derivation is replicated here only because configured (`p_`) initialization is
    /// crate-private; it is checked against the public v2 factory derivation and deal first.
    pub fn opening(directory: &Path, excluded: Option<&str>) -> Result<Opening> {
        let values = serde_json::from_slice::<BTreeMap<String, String>>(&std::fs::read(
            directory.join("fixture-config.json"),
        )?)?;
        let config = ServerConfig::from_values(&values)?;
        let matches = MatchFactory::new(&config)?;
        let key = hmac::Key::new(hmac::HMAC_SHA256, &std::fs::read(&config.seed_file)?);
        let requested_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let (legacy, _) = scrabble_server::dictionary::load(
            config.ruleset_file.as_deref(),
            config.dictionary.as_ref(),
        )?;
        let probe = "0".repeat(32);
        let probe_id = matches.board_creation_id(&probe, requested_at)?;
        if creation_id(&key, b"scrabble/create/v2\0", "b", &probe, requested_at)? != probe_id {
            return Err("fixture creation derivation drifted from the native factory".into());
        }
        let probe_state = deal(&probe_id, &legacy, &key)?;
        let mut native = matches.create(&probe_id)?;
        start_with_board(&mut native, &probe_id, &probe)?;
        for seat in [2_u32, 3] {
            let own: scrabble_game::protocol::PlayerSnapshot = serde_json::from_slice(
                &game_server::GameSimulation::snapshot_for(&native, seat)?.payload,
            )?;
            let expected: Vec<_> = probe_state
                .rack(PlayerId::new(u64::from(seat)))
                .ok_or("missing probe rack")?
                .iter()
                .map(|tile| tile.face())
                .collect();
            let actual: Vec<_> = own
                .own_rack
                .tiles
                .iter()
                .map(|tile| tile.face.clone())
                .collect();
            if expected != actual {
                return Err("fixture deal derivation drifted from the native factory".into());
            }
        }
        let path = config
            .profile_catalog_file
            .as_ref()
            .ok_or("missing catalog")?;
        let document: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let words = row(&document, "wordLists", WORDS)?;
        let text = std::fs::read_to_string(
            path.parent()
                .unwrap_or(Path::new("."))
                .join(words["file"].as_str().ok_or("missing word file")?),
        )?;
        let words = WordListProfile::new(WordList::from_text(
            serde_json::from_value::<ContentIdentity>(words["identity"].clone())?,
            &text,
        )?)?;
        let tiles = row(&document, "tileProfiles", TILES)?;
        let tiles = TileProfile::new(
            serde_json::from_value(tiles["identity"].clone())?,
            serde_json::from_value::<Vec<TileDefinition>>(tiles["tiles"].clone())?,
        )?;
        let board = row(&document, "boardProfiles", BOARD)?;
        let board = BoardProfile::new(
            serde_json::from_value(board["identity"].clone())?,
            serde_json::from_value(board["boardSize"].clone())?,
            serde_json::from_value::<Vec<Premium>>(board["premiums"].clone())?,
        )?;
        let rules = profiles::compose_profiles(
            &words,
            &tiles,
            &board,
            MatchRules {
                rack_size: legacy.rack_size,
                bingo_bonus: legacy.bingo_bonus,
                exchange_minimum_bag: legacy.exchange_minimum_bag,
                scoreless_turn_limit: legacy.scoreless_turn_limit,
                minimum_players: legacy.minimum_players,
                maximum_players: legacy.maximum_players,
            },
        )?
        .rules;
        let letters = |letters: &str| TileFace::Letters {
            letters: letters.into(),
        };
        let letter = |letter: char| TileFace::Letter { letter };
        for nonce in 0..200_000_u32 {
            let request_id = format!("{nonce:032x}");
            if excluded == Some(request_id.as_str()) {
                continue;
            }
            let id = creation_id(
                &key,
                b"scrabble/create/v3\0",
                "p",
                &request_id,
                requested_at,
            )?;
            let state = deal(&id, &rules, &key)?;
            let ada = state.rack(PlayerId::new(2)).ok_or("missing rack")?;
            let lin = state.rack(PlayerId::new(3)).ok_or("missing rack")?;
            if has(ada, &letters("CH"))
                && has(ada, &letter('A'))
                && has(ada, &letter('T'))
                && has(ada, &TileFace::Blank {})
                && has(lin, &letters("SCH"))
                && has(lin, &letter('A'))
            {
                return Ok(Opening {
                    request_id,
                    requested_at,
                });
            }
        }
        Err("could not select a native multi-letter opening".into())
    }

    /// Claim the board (seat 1), name Ada (2) and Lin (3), and start: the browser journey's order.
    fn start_with_board(
        simulation: &mut scrabble_server::simulation::ScrabbleSimulation,
        id: &game_server::MatchId,
        request_id: &str,
    ) -> Result<()> {
        for player in 1..=3 {
            simulation.add_player(player)?;
        }
        for (player, sequence, command) in [
            (
                1,
                1,
                Command::ClaimBoard {
                    request_id: request_id.into(),
                },
            ),
            (
                2,
                1,
                Command::SetName {
                    display_name: "Ada".into(),
                },
            ),
            (
                3,
                1,
                Command::SetName {
                    display_name: "Lin".into(),
                },
            ),
            (1, 2, Command::Start {}),
        ] {
            let payload = serde_json::to_vec(&CommandEnvelope {
                version: ProtocolVersion,
                game_id: factory::game_id(id),
                player_id: PlayerId::new(u64::from(player)),
                sequence,
                expected_turn: TurnId::new(0),
                command,
            })?;
            simulation.apply_command(player, sequence, &payload)?;
        }
        Ok(())
    }
}
