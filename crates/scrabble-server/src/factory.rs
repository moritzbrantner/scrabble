//! Persistent private initialization and stable game identity; no transport/session implementation.
use crate::{config::ServerConfig, simulation::ScrabbleSimulation};
use game_server::MatchId;
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};
use scrabble_game::{identity::GameId, ruleset::english_fixture};
use std::{
    collections::BTreeSet,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

type ErrorResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
fn private_file(path: &Path) -> ErrorResult<[u8; 32]> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() != 32 {
        return Err("seed file must be a regular 32-byte private file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("seed file must not be accessible to group or other users".into());
        }
    }
    let mut seed = [0; 32];
    fs::File::open(path)?.read_exact(&mut seed)?;
    Ok(seed)
}
fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
/// Atomically publishes a complete key without overwriting a concurrently created key.
fn create_seed(path: &Path, seed: &[u8; 32]) -> ErrorResult<[u8; 32]> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path.file_name().ok_or("seed path must name a file")?;
    let mut temp_name = name.to_os_string();
    temp_name.push(".tmp");
    let temp = parent.join(temp_name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    let result = (|| -> io::Result<()> {
        file.write_all(seed)?;
        file.sync_all()?;
        match fs::hard_link(&temp, path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        sync_directory(parent)
    })();
    drop(file);
    fs::remove_file(&temp)?;
    result?;
    sync_directory(parent)?;
    private_file(path)
}
fn seed(config: &ServerConfig) -> ErrorResult<[u8; 32]> {
    match fs::symlink_metadata(&config.seed_file) {
        Ok(_) => {
            let seed = private_file(&config.seed_file)?;
            if config.test_seed.is_some_and(|test| seed != test) {
                return Err(
                    "test seed differs from persisted initialization; refusing to replace it"
                        .into(),
                );
            }
            Ok(seed)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if config.recovery_directory.exists() {
                return Err(
                    "recovery exists without its initialization seed; refusing new initialization"
                        .into(),
                );
            }
            let mut seed = config.test_seed.unwrap_or([0; 32]);
            if config.test_seed.is_none() {
                SystemRandom::new()
                    .fill(&mut seed)
                    .map_err(|_| "secure seed generation failed")?;
            }
            create_seed(&config.seed_file, &seed)
        }
        Err(error) => Err(error.into()),
    }
}

/// Shared private initialization for both configured and dynamically created matches.
/// The factory retains a derivation key, never a copy of any authoritative game state.
#[derive(Clone)]
pub struct MatchFactory {
    key: hmac::Key,
}
impl MatchFactory {
    pub fn new(config: &ServerConfig) -> ErrorResult<Self> {
        let mut identities = BTreeSet::new();
        for id in &config.match_ids {
            if !identities.insert(game_id(id)) {
                return Err("configured matches have colliding game identities".into());
            }
        }
        Ok(Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, &seed(config)?),
        })
    }

    pub fn create(&self, id: &MatchId) -> ErrorResult<ScrabbleSimulation> {
        let mut derivation = hmac::Context::with_key(&self.key);
        derivation.update(b"scrabble/deal/v1\0");
        derivation.update(id.as_str().as_bytes());
        let seed: [u8; 32] = derivation.sign().as_ref().try_into()?;
        Ok(ScrabbleSimulation::new(
            game_id(id),
            english_fixture(),
            seed,
        )?)
    }
}

pub fn game_id(id: &MatchId) -> GameId {
    let mut identity = digest::Context::new(&digest::SHA256);
    identity.update(b"scrabble/game-id/v1\0");
    identity.update(id.as_str().as_bytes());
    let digest = identity.finish();
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&digest.as_ref()[..8]);
    GameId::new(u64::from_be_bytes(bytes))
}

pub fn create_matches(config: &ServerConfig) -> ErrorResult<Vec<(MatchId, ScrabbleSimulation)>> {
    let factory = MatchFactory::new(config)?;
    let mut matches = Vec::with_capacity(config.match_ids.len());
    for id in &config.match_ids {
        matches.push((id.clone(), factory.create(id)?));
    }
    Ok(matches)
}
