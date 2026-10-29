//! Durable guest-owned private tile profiles (#100; Decisions 1.21, 1.25, 1.27, 1.29, 1.45).
//!
//! Each original is one private JSON file holding a keyed verifier of its owner
//! capability (never the capability), its current name and its recent immutable
//! versions. Tiles are validated by the same `TileProfile` constructor as operator
//! catalog profiles, so no second rules engine or new bound exists. Tile profiles are
//! private: there are no share links (Decision 1.27). Started matches never read this
//! library again: their configured initialization manifest pins the composed rules.
use crate::{
    ErrorResult,
    account_link::{self, AccountIndex, Linked, LinkedLibrary},
    dictionary::hexadecimal,
    layout_library::{is_hex_id, random_hex, sync_directory},
    profiles::{ProfileError, TileProfile},
};
use ring::hmac;
use scrabble_game::ruleset::{ContentIdentity, TileDefinition};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::PathBuf,
    sync::Mutex,
};

/// Upper bound of originals kept by one server; protects disk use of anonymous guests.
const MAX_PROFILES: usize = 4096;
/// Recent immutable versions of one original that remain selectable for new matches.
const MAX_VERSIONS: usize = 16;
const MAX_NAME_CHARS: usize = 64;
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug)]
pub enum TileProfileError {
    Invalid(String),
    OwnerRejected,
    /// The original is linked to a different account (#132).
    LinkedElsewhere,
    /// The verified account is not this original's linked owner (#132).
    NotAccountOwner,
    Unknown,
    Full,
    Internal,
}
impl From<io::Error> for TileProfileError {
    fn from(_: io::Error) -> Self {
        Self::Internal
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredVersion {
    revision: u64,
    /// Random per-version source identity; the public tile identity is its content hash.
    source: ContentIdentity,
    tiles: Vec<TileDefinition>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredProfile {
    version: u8,
    profile_id: String,
    verifier: String,
    name: String,
    /// Linked Moenarch account (#132); `None` while guest-owned only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    /// Verifiers of device capabilities issued to the linked account by `reconnect`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    device_verifiers: Vec<String>,
    versions: Vec<StoredVersion>,
}
impl StoredProfile {
    fn current(&self) -> &StoredVersion {
        self.versions
            .last()
            .expect("a stored tile profile has a version")
    }
}

/// A saved version as returned to its owner.
pub struct TileProfileView {
    pub profile_id: String,
    pub name: String,
    pub revision: u64,
    pub identity: ContentIdentity,
    pub tiles: Vec<TileDefinition>,
}
pub struct Created {
    pub view: TileProfileView,
    pub owner_capability: String,
}

#[derive(Default)]
struct Index {
    /// Tile identity name of a retained version -> its original.
    tiles: HashMap<String, String>,
    accounts: AccountIndex,
    profiles: usize,
}

pub struct TileProfileLibrary {
    directory: PathBuf,
    key: hmac::Key,
    index: Mutex<Index>,
}

pub fn is_tile_profile_id(value: &str) -> bool {
    is_hex_id(value, "t_")
}

/// Validates a name and tiles without saving anything.
pub fn validate(name: &str, tiles: &[TileDefinition]) -> Result<(), TileProfileError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed != name
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(TileProfileError::Invalid(format!(
            "tile profile name must be 1 to {MAX_NAME_CHARS} characters without surrounding spaces"
        )));
    }
    tile_profile(
        &ContentIdentity {
            name: "custom-validation".into(),
            revision: "1".into(),
        },
        tiles,
    )
    .map(|_| ())
}
fn tile_profile(
    source: &ContentIdentity,
    tiles: &[TileDefinition],
) -> Result<TileProfile, TileProfileError> {
    TileProfile::new(source.clone(), tiles.to_vec()).map_err(|error| match error {
        ProfileError::InvalidTiles(reason) => TileProfileError::Invalid(format!(
            "{reason}: every face is unique, counts are positive, at most one blank row with 0 points, at most 64 rows and 200 tiles"
        )),
        other => TileProfileError::Invalid(other.to_string()),
    })
}

impl TileProfileLibrary {
    /// Opens (creating if needed) the private library directory and indexes it.
    pub fn open(directory: PathBuf, derivation: &hmac::Key) -> ErrorResult<Self> {
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let key = hmac::Key::new(
            hmac::HMAC_SHA256,
            hmac::sign(derivation, b"scrabble/tile-profile-owner/v1\0").as_ref(),
        );
        let library = Self {
            directory,
            key,
            index: Mutex::new(Index::default()),
        };
        let mut index = Index::default();
        for entry in fs::read_dir(&library.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
                continue;
            };
            if !is_tile_profile_id(id) {
                continue;
            }
            let stored = library
                .load(id)?
                .ok_or("tile profile disappeared while indexing")?;
            Self::insert(&mut index, &stored);
        }
        *library
            .index
            .lock()
            .map_err(|_| "tile profile index unavailable")? = index;
        Ok(library)
    }
    fn insert(index: &mut Index, stored: &StoredProfile) {
        for version in &stored.versions {
            index
                .tiles
                .insert(version.source.name.clone(), stored.profile_id.clone());
        }
        index
            .accounts
            .add(stored.account_id.as_deref(), &stored.profile_id);
        index.profiles += 1;
    }
    fn path(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }
    fn load(&self, id: &str) -> ErrorResult<Option<StoredProfile>> {
        let path = self.path(id);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() {
            return Err("tile profile entries must be regular files".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("tile profile entry exceeds its supported size".into());
        }
        let stored: StoredProfile = serde_json::from_slice(&bytes)?;
        if stored.version != 1
            || stored.profile_id != id
            || stored.versions.is_empty()
            || !account_link::valid_persisted(
                stored.account_id.as_deref(),
                &stored.device_verifiers,
            )
        {
            return Err("invalid persisted tile profile entry".into());
        }
        Ok(Some(stored))
    }
    fn store(&self, stored: &StoredProfile) -> Result<(), TileProfileError> {
        let bytes = serde_json::to_vec(stored).map_err(|_| TileProfileError::Internal)?;
        let suffix = random_hex(8).map_err(|_| TileProfileError::Internal)?;
        let temporary = self
            .directory
            .join(format!(".{}.{suffix}.tmp", stored.profile_id));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> io::Result<()> {
            let mut file = options.open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, self.path(&stored.profile_id))?;
            sync_directory(&self.directory)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        Ok(result?)
    }
    fn owner_message(profile_id: &str, capability: &str) -> Vec<u8> {
        let mut message = Vec::with_capacity(profile_id.len() + 1 + capability.len());
        message.extend_from_slice(profile_id.as_bytes());
        message.push(0);
        message.extend_from_slice(capability.as_bytes());
        message
    }
    /// Loads an original for its owner; the capability check is constant-time.
    fn owned(&self, profile_id: &str, capability: &str) -> Result<StoredProfile, TileProfileError> {
        if !is_tile_profile_id(profile_id) {
            return Err(TileProfileError::Unknown);
        }
        let stored = self
            .load(profile_id)
            .map_err(|_| TileProfileError::Internal)?
            .ok_or(TileProfileError::Unknown)?;
        let matched = account_link::owner_matches(
            &self.key,
            &Self::owner_message(profile_id, capability),
            &stored.verifier,
            &stored.device_verifiers,
        )
        .ok_or(TileProfileError::Internal)?;
        if !matched {
            return Err(TileProfileError::OwnerRejected);
        }
        Ok(stored)
    }
    fn view(stored: &StoredProfile) -> Result<TileProfileView, TileProfileError> {
        let current = stored.current();
        Ok(TileProfileView {
            profile_id: stored.profile_id.clone(),
            name: stored.name.clone(),
            revision: current.revision,
            identity: tile_profile(&current.source, &current.tiles)?
                .identity()
                .clone(),
            tiles: current.tiles.clone(),
        })
    }
    fn new_source(revision: u64) -> Result<ContentIdentity, TileProfileError> {
        Ok(ContentIdentity {
            name: format!(
                "custom-tiles-{}",
                random_hex(16).map_err(|_| TileProfileError::Internal)?
            ),
            revision: revision.to_string(),
        })
    }

    pub fn create(
        &self,
        name: &str,
        tiles: Vec<TileDefinition>,
    ) -> Result<Created, TileProfileError> {
        validate(name, &tiles)?;
        let mut index = self.index.lock().map_err(|_| TileProfileError::Internal)?;
        if index.profiles >= MAX_PROFILES {
            return Err(TileProfileError::Full);
        }
        let random = |bytes| random_hex(bytes).map_err(|_| TileProfileError::Internal);
        let profile_id = format!("t_{}", random(16)?);
        let capability = random(32)?;
        let stored = StoredProfile {
            version: 1,
            verifier: hexadecimal(
                hmac::sign(&self.key, &Self::owner_message(&profile_id, &capability)).as_ref(),
            ),
            profile_id,
            name: name.to_owned(),
            account_id: None,
            device_verifiers: Vec::new(),
            versions: vec![StoredVersion {
                revision: 1,
                source: Self::new_source(1)?,
                tiles,
            }],
        };
        let view = Self::view(&stored)?;
        self.store(&stored)?;
        Self::insert(&mut index, &stored);
        Ok(Created {
            view,
            owner_capability: capability,
        })
    }
    pub fn read(
        &self,
        profile_id: &str,
        capability: &str,
    ) -> Result<TileProfileView, TileProfileError> {
        let _index = self.index.lock().map_err(|_| TileProfileError::Internal)?;
        Self::view(&self.owned(profile_id, capability)?)
    }
    pub fn update(
        &self,
        profile_id: &str,
        capability: &str,
        name: &str,
        tiles: Vec<TileDefinition>,
    ) -> Result<TileProfileView, TileProfileError> {
        let mut index = self.index.lock().map_err(|_| TileProfileError::Internal)?;
        let mut stored = self.owned(profile_id, capability)?;
        validate(name, &tiles)?;
        let revision = stored.current().revision + 1;
        stored.name = name.to_owned();
        stored.versions.push(StoredVersion {
            revision,
            source: Self::new_source(revision)?,
            tiles,
        });
        let dropped: Vec<_> = if stored.versions.len() > MAX_VERSIONS {
            let excess = stored.versions.len() - MAX_VERSIONS;
            stored.versions.drain(..excess).collect()
        } else {
            Vec::new()
        };
        let view = Self::view(&stored)?;
        self.store(&stored)?;
        for version in dropped {
            index.tiles.remove(&version.source.name);
        }
        index.tiles.insert(
            stored.current().source.name.clone(),
            stored.profile_id.clone(),
        );
        Ok(view)
    }
    pub fn delete(&self, profile_id: &str, capability: &str) -> Result<(), TileProfileError> {
        let mut index = self.index.lock().map_err(|_| TileProfileError::Internal)?;
        let stored = self.owned(profile_id, capability)?;
        fs::remove_file(self.path(profile_id))?;
        sync_directory(&self.directory)?;
        for version in &stored.versions {
            index.tiles.remove(&version.source.name);
        }
        index
            .accounts
            .remove(stored.account_id.as_deref(), &stored.profile_id);
        index.profiles = index.profiles.saturating_sub(1);
        Ok(())
    }
    /// Links an owned original to a verified account (#132). Linking again to the same
    /// account is idempotent; another account's link is never replaced.
    pub fn link(
        &self,
        profile_id: &str,
        capability: &str,
        account_id: &str,
    ) -> Result<Linked, TileProfileError> {
        let mut index = self.index.lock().map_err(|_| TileProfileError::Internal)?;
        let mut stored = self.owned(profile_id, capability)?;
        let linked = account_link::link(&mut stored.account_id, account_id)
            .map_err(|()| TileProfileError::LinkedElsewhere)?;
        if linked == Linked::New {
            self.store(&stored)?;
            index.accounts.add(Some(account_id), profile_id);
        }
        Ok(linked)
    }
    /// Originals linked to `account_id`.
    pub fn linked(&self, account_id: &str) -> Result<Vec<LinkedLibrary>, TileProfileError> {
        let index = self.index.lock().map_err(|_| TileProfileError::Internal)?;
        let mut linked = Vec::new();
        for id in index.accounts.ids(account_id) {
            if let Some(stored) = self.load(&id).map_err(|_| TileProfileError::Internal)? {
                linked.push(LinkedLibrary {
                    id,
                    name: stored.name,
                });
            }
        }
        Ok(linked)
    }
    /// Issues a new device capability for an original linked to `account_id`; the
    /// original guest capability and earlier devices keep working.
    pub fn reconnect(
        &self,
        profile_id: &str,
        account_id: &str,
    ) -> Result<String, TileProfileError> {
        let _index = self.index.lock().map_err(|_| TileProfileError::Internal)?;
        if !is_tile_profile_id(profile_id) {
            return Err(TileProfileError::Unknown);
        }
        let mut stored = self
            .load(profile_id)
            .map_err(|_| TileProfileError::Internal)?
            .ok_or(TileProfileError::Unknown)?;
        if stored.account_id.as_deref() != Some(account_id) {
            return Err(TileProfileError::NotAccountOwner);
        }
        let capability = random_hex(32).map_err(|_| TileProfileError::Internal)?;
        account_link::add_device(
            &mut stored.device_verifiers,
            hexadecimal(
                hmac::sign(&self.key, &Self::owner_message(profile_id, &capability)).as_ref(),
            ),
        );
        self.store(&stored)?;
        Ok(capability)
    }
    /// Resolves a retained saved version for match setup. Unknown, forged, dropped or
    /// deleted identities are `None`.
    pub fn tiles(&self, identity: &ContentIdentity) -> Option<TileProfile> {
        let index = self.index.lock().ok()?;
        let profile_id = index.tiles.get(&identity.name)?;
        let stored = self.load(profile_id).ok()??;
        stored
            .versions
            .iter()
            .filter(|version| version.source.name == identity.name)
            .filter_map(|version| tile_profile(&version.source, &version.tiles).ok())
            .find(|profile| profile.identity() == identity)
    }
}
