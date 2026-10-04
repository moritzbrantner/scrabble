//! Real Scrabble application for browser creation acceptance; no replacement game/session rules.
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
use std::{collections::BTreeMap, error::Error, path::PathBuf};
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
