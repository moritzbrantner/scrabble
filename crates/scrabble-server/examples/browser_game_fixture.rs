//! Real Scrabble application for browser creation acceptance; no replacement game/session rules.
use scrabble_server::{config::ServerConfig, hosting};
use std::{collections::BTreeMap, error::Error, path::PathBuf};
use tokio::sync::mpsc;
use wtransport::Identity;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
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
    let config = ServerConfig::from_values(&BTreeMap::from([
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
    ]))?;
    let metadata = serde_json::json!({ "endpoint": format!("https://127.0.0.1:{port}/game"),
        "api": format!("http://127.0.0.1:{api_port}"), "status": format!("http://127.0.0.1:{status_port}"),
        "certificateHash": identity.certificate_chain().as_slice()[0].hash().as_ref() });
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
