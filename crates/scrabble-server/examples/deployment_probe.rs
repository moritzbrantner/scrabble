//! Probe the actual container UDP listener using an owned local test certificate.
use game_server::{SnapshotReassembler, decode_welcome};
use scrabble_game::protocol::PlayerSnapshot;
use std::{error::Error, time::Duration};
use tokio::time::timeout;
use wtransport::{ClientConfig, Endpoint, endpoint::ConnectOptions, tls::Certificate};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err(
            "expected local test certificate, match endpoint, and public join metadata".into(),
        );
    }
    let certificate = Certificate::load_pemfile(&args[1]).await?;
    let metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let game_id = metadata["gameId"]
        .as_str()
        .ok_or("missing public game identity")?;
    let client = Endpoint::client(
        ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([certificate.hash()])
            .build(),
    )?;
    for origin in [None, Some("https://wrong.example")] {
        let mut options = ConnectOptions::builder(&args[2]);
        if let Some(origin) = origin {
            options = options.add_header("origin", origin);
        }
        let result = timeout(Duration::from_secs(5), client.connect(options.build())).await?;
        if !matches!(
            result,
            Err(wtransport::error::ConnectingError::SessionRejected)
        ) {
            return Err("production transport did not reject undeclared browser origin".into());
        }
    }
    let options = ConnectOptions::builder(&args[2])
        .add_header("origin", "https://moritzbrantner.github.io")
        .build();
    let connection = timeout(Duration::from_secs(5), client.connect(options)).await??;
    let mut stream = timeout(Duration::from_secs(5), connection.accept_uni()).await??;
    let mut bytes = [0; 46];
    stream.read_exact(&mut bytes).await?;
    let welcome = decode_welcome(&bytes)?;
    if welcome.player_id != 1 {
        return Err("rejected origins consumed an admission identity".into());
    }
    timeout(Duration::from_secs(5), async {
        let mut reassembler = SnapshotReassembler::new();
        loop {
            let bytes = connection.receive_datagram().await?;
            if let Some(frame) = reassembler.accept(&bytes)? {
                let snapshot: PlayerSnapshot = serde_json::from_slice(&frame.payload)?;
                if serde_json::to_value(snapshot.public.game_id)?.as_str() != Some(game_id) {
                    return Err::<(), Box<dyn Error + Send + Sync>>(
                        "transport reached a different authoritative game".into(),
                    );
                }
                return Ok(());
            }
        }
    })
    .await??;
    connection.close(0u32.into(), b"deployment probe complete");
    println!("Container WebTransport origin enforcement and authoritative snapshot passed");
    Ok(())
}
