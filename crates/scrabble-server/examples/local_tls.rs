//! Explicit local-only TLS provisioning; never overwrites existing identity files.
use scrabble_server::config::{Mode, ServerConfig};
use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use wtransport::Identity;

fn write_new(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if private {
            options.mode(0o600);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = private;
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = ServerConfig::from_env()?;
    if !matches!(config.mode, Mode::Local) {
        return Err("local TLS provisioning is forbidden in production".into());
    }
    let identity = match (config.certificate.exists(), config.private_key.exists()) {
        (true, true) => Identity::load_pemfiles(&config.certificate, &config.private_key).await?,
        (false, false) => {
            let identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"])?;
            write_new(&config.private_key, identity.private_key().to_secret_pem().as_bytes(), true)?;
            write_new(&config.certificate, identity.certificate_chain().as_slice()[0].to_pem().as_bytes(), false)?;
            identity
        },
        _ => return Err("incomplete local TLS identity; supply both files or remove the unused local identity explicitly".into()),
    };
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "endpoint": format!("https://127.0.0.1:{}{}", config.port, config.route_prefix.as_str()),
            "certificateHash": identity.certificate_chain().as_slice()[0].hash().as_ref(),
        }))?
    );
    Ok(())
}
