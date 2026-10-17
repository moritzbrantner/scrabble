//! Durable guest-owned custom board layouts (#101; Decisions 1.33, 1.35, 1.37, 1.45).
//!
//! Each original is one private JSON file holding a keyed verifier of its owner
//! capability (never the capability), its current name, an optional share id and its
//! recent immutable versions. Geometry is validated by the same `BoardProfile`
//! constructor as operator catalog boards, so no second rules engine or new bound
//! exists. Started matches never read this library again: their configured
//! initialization manifest pins the composed rules.
use crate::{
    ErrorResult,
    dictionary::hexadecimal,
    profiles::{BoardProfile, ProfileError},
};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use scrabble_game::ruleset::{
    BoardShape, ContentIdentity, Premium, PremiumPolicy, WordLengthMetric,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Upper bound of originals kept by one server; protects disk use of anonymous guests.
const MAX_LAYOUTS: usize = 4096;
/// Recent immutable versions of one original that remain selectable for new matches.
const MAX_VERSIONS: usize = 16;
const MAX_NAME_CHARS: usize = 64;
const MAX_FILE_BYTES: u64 = 512 * 1024;

/// The public layout document, using the catalog's board field names.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Layout {
    pub board_shape: BoardShape,
    pub board_size: u8,
    pub premiums: Vec<Premium>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub premium_policy: Option<PremiumPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_word_length: Option<WordLengthMetric>,
}

#[derive(Debug)]
pub enum LayoutError {
    Invalid(String),
    OwnerRejected,
    Unknown,
    Full,
    Internal,
}
impl From<io::Error> for LayoutError {
    fn from(_: io::Error) -> Self {
        Self::Internal
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredVersion {
    revision: u64,
    /// Random per-version source identity; the public board identity is its content hash.
    source: ContentIdentity,
    layout: Layout,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredLayout {
    version: u8,
    layout_id: String,
    verifier: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    share_id: Option<String>,
    versions: Vec<StoredVersion>,
}
impl StoredLayout {
    fn current(&self) -> &StoredVersion {
        self.versions.last().expect("a stored layout has a version")
    }
}

/// A saved version as returned to clients.
pub struct LayoutView {
    pub layout_id: String,
    pub name: String,
    pub revision: u64,
    pub board: ContentIdentity,
    pub layout: Layout,
    pub share_id: Option<String>,
}
pub struct Created {
    pub view: LayoutView,
    pub owner_capability: String,
}

#[derive(Default)]
struct Index {
    /// Board identity name of a retained version -> its original.
    boards: HashMap<String, String>,
    shares: HashMap<String, String>,
    layouts: usize,
}

pub struct LayoutLibrary {
    directory: PathBuf,
    key: hmac::Key,
    index: Mutex<Index>,
}

pub(crate) fn random_hex(bytes: usize) -> Result<String, LayoutError> {
    let mut value = vec![0; bytes];
    SystemRandom::new()
        .fill(&mut value)
        .map_err(|_| LayoutError::Internal)?;
    Ok(hexadecimal(&value))
}
pub(crate) fn is_hex_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|rest| {
        rest.len() == 32
            && rest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub fn is_layout_id(value: &str) -> bool {
    is_hex_id(value, "l_")
}
pub fn is_share_id(value: &str) -> bool {
    is_hex_id(value, "s_")
}
pub(crate) fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

/// Validates a name and layout without saving anything. Bounded: the document is
/// already limited by the API body cap and the existing board limits.
pub fn validate(name: &str, layout: &Layout) -> Result<(), LayoutError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed != name
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(LayoutError::Invalid(format!(
            "layout name must be 1 to {MAX_NAME_CHARS} characters without surrounding spaces"
        )));
    }
    board_profile(
        &ContentIdentity {
            name: "custom-validation".into(),
            revision: "1".into(),
        },
        layout,
    )
    .map(|_| ())
}
fn board_profile(source: &ContentIdentity, layout: &Layout) -> Result<BoardProfile, LayoutError> {
    let (policy, metric) = match layout.board_shape {
        BoardShape::Square => {
            if layout.premium_policy.is_some() || layout.main_word_length.is_some() {
                return Err(LayoutError::Invalid(
                    "premium application settings apply only to hexagonal boards".into(),
                ));
            }
            (PremiumPolicy::AllWord, WordLengthMetric::Tiles)
        }
        BoardShape::Hexagon => (
            layout.premium_policy.unwrap_or_default(),
            layout.main_word_length.unwrap_or_default(),
        ),
    };
    BoardProfile::with_geometry(
        source.clone(),
        layout.board_size,
        layout.premiums.clone(),
        layout.board_shape,
        policy,
        metric,
    )
    .map_err(|error| match error {
        ProfileError::InvalidBoard(reason) => LayoutError::Invalid(format!(
            "{reason}: boards are complete regular grids with an odd side (square up to 15, hexagon 3 to 15) and one premium per on-board cell"
        )),
        other => LayoutError::Invalid(other.to_string()),
    })
}
/// Hex layouts always state both policy fields; squares never carry them.
fn normalized(mut layout: Layout) -> Layout {
    if layout.board_shape == BoardShape::Hexagon {
        layout.premium_policy = Some(layout.premium_policy.unwrap_or_default());
        layout.main_word_length = Some(layout.main_word_length.unwrap_or_default());
    }
    layout
}

impl LayoutLibrary {
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
            hmac::sign(derivation, b"scrabble/board-layout-owner/v1\0").as_ref(),
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
            if !is_layout_id(id) {
                continue;
            }
            let stored = library
                .load(id)?
                .ok_or("layout disappeared while indexing")?;
            Self::insert(&mut index, &stored)?;
        }
        *library
            .index
            .lock()
            .map_err(|_| "layout index unavailable")? = index;
        Ok(library)
    }
    fn insert(index: &mut Index, stored: &StoredLayout) -> ErrorResult<()> {
        for version in &stored.versions {
            index
                .boards
                .insert(version.source.name.clone(), stored.layout_id.clone());
        }
        if let Some(share) = &stored.share_id {
            index.shares.insert(share.clone(), stored.layout_id.clone());
        }
        index.layouts += 1;
        Ok(())
    }
    fn path(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }
    fn load(&self, id: &str) -> ErrorResult<Option<StoredLayout>> {
        let path = self.path(id);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() {
            return Err("layout entries must be regular files".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("layout entry exceeds its supported size".into());
        }
        let stored: StoredLayout = serde_json::from_slice(&bytes)?;
        if stored.version != 1
            || stored.layout_id != id
            || stored.versions.is_empty()
            || stored.share_id.as_deref().is_some_and(|s| !is_share_id(s))
        {
            return Err("invalid persisted layout entry".into());
        }
        Ok(Some(stored))
    }
    fn store(&self, stored: &StoredLayout) -> Result<(), LayoutError> {
        let bytes = serde_json::to_vec(stored).map_err(|_| LayoutError::Internal)?;
        let temporary =
            self.directory
                .join(format!(".{}.{}.tmp", stored.layout_id, random_hex(8)?));
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
            fs::rename(&temporary, self.path(&stored.layout_id))?;
            sync_directory(&self.directory)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        Ok(result?)
    }
    fn owner_message(layout_id: &str, capability: &str) -> Vec<u8> {
        let mut message = Vec::with_capacity(layout_id.len() + 1 + capability.len());
        message.extend_from_slice(layout_id.as_bytes());
        message.push(0);
        message.extend_from_slice(capability.as_bytes());
        message
    }
    fn verifier(&self, layout_id: &str, capability: &str) -> hmac::Tag {
        hmac::sign(&self.key, &Self::owner_message(layout_id, capability))
    }
    /// Loads an original for its owner; the capability check is constant-time.
    fn owned(&self, layout_id: &str, capability: &str) -> Result<StoredLayout, LayoutError> {
        if !is_layout_id(layout_id) {
            return Err(LayoutError::Unknown);
        }
        let stored = self
            .load(layout_id)
            .map_err(|_| LayoutError::Internal)?
            .ok_or(LayoutError::Unknown)?;
        let expected = decode_hex(&stored.verifier).ok_or(LayoutError::Internal)?;
        // `hmac::verify` recomputes the tag and compares it in constant time.
        hmac::verify(
            &self.key,
            &Self::owner_message(layout_id, capability),
            &expected,
        )
        .map_err(|_| LayoutError::OwnerRejected)?;
        Ok(stored)
    }
    fn view(&self, stored: &StoredLayout) -> Result<LayoutView, LayoutError> {
        let current = stored.current();
        Ok(LayoutView {
            layout_id: stored.layout_id.clone(),
            name: stored.name.clone(),
            revision: current.revision,
            board: board_profile(&current.source, &current.layout)?
                .identity()
                .clone(),
            layout: current.layout.clone(),
            share_id: stored.share_id.clone(),
        })
    }
    fn new_source(revision: u64) -> Result<ContentIdentity, LayoutError> {
        Ok(ContentIdentity {
            name: format!("custom-{}", random_hex(16)?),
            revision: revision.to_string(),
        })
    }

    pub fn create(&self, name: &str, layout: Layout) -> Result<Created, LayoutError> {
        validate(name, &layout)?;
        let mut index = self.index.lock().map_err(|_| LayoutError::Internal)?;
        if index.layouts >= MAX_LAYOUTS {
            return Err(LayoutError::Full);
        }
        let layout_id = format!("l_{}", random_hex(16)?);
        let capability = random_hex(32)?;
        let stored = StoredLayout {
            version: 1,
            verifier: hexadecimal(self.verifier(&layout_id, &capability).as_ref()),
            layout_id,
            name: name.to_owned(),
            share_id: None,
            versions: vec![StoredVersion {
                revision: 1,
                source: Self::new_source(1)?,
                layout: normalized(layout),
            }],
        };
        let view = self.view(&stored)?;
        self.store(&stored)?;
        Self::insert(&mut index, &stored).map_err(|_| LayoutError::Internal)?;
        Ok(Created {
            view,
            owner_capability: capability,
        })
    }
    pub fn read(&self, layout_id: &str, capability: &str) -> Result<LayoutView, LayoutError> {
        let _index = self.index.lock().map_err(|_| LayoutError::Internal)?;
        let stored = self.owned(layout_id, capability)?;
        self.view(&stored)
    }
    pub fn update(
        &self,
        layout_id: &str,
        capability: &str,
        name: &str,
        layout: Layout,
    ) -> Result<LayoutView, LayoutError> {
        let mut index = self.index.lock().map_err(|_| LayoutError::Internal)?;
        let mut stored = self.owned(layout_id, capability)?;
        validate(name, &layout)?;
        let revision = stored.current().revision + 1;
        stored.name = name.to_owned();
        stored.versions.push(StoredVersion {
            revision,
            source: Self::new_source(revision)?,
            layout: normalized(layout),
        });
        let dropped: Vec<_> = if stored.versions.len() > MAX_VERSIONS {
            let excess = stored.versions.len() - MAX_VERSIONS;
            stored.versions.drain(..excess).collect()
        } else {
            Vec::new()
        };
        let view = self.view(&stored)?;
        self.store(&stored)?;
        for version in dropped {
            index.boards.remove(&version.source.name);
        }
        index.boards.insert(
            stored.current().source.name.clone(),
            stored.layout_id.clone(),
        );
        Ok(view)
    }
    pub fn delete(&self, layout_id: &str, capability: &str) -> Result<(), LayoutError> {
        let mut index = self.index.lock().map_err(|_| LayoutError::Internal)?;
        let stored = self.owned(layout_id, capability)?;
        fs::remove_file(self.path(layout_id))?;
        sync_directory(&self.directory)?;
        for version in &stored.versions {
            index.boards.remove(&version.source.name);
        }
        if let Some(share) = &stored.share_id {
            index.shares.remove(share);
        }
        index.layouts = index.layouts.saturating_sub(1);
        Ok(())
    }
    /// Enables (idempotently) the original's share link.
    pub fn share(&self, layout_id: &str, capability: &str) -> Result<String, LayoutError> {
        let mut index = self.index.lock().map_err(|_| LayoutError::Internal)?;
        let mut stored = self.owned(layout_id, capability)?;
        if let Some(share) = &stored.share_id {
            return Ok(share.clone());
        }
        let share = format!("s_{}", random_hex(16)?);
        stored.share_id = Some(share.clone());
        self.store(&stored)?;
        index.shares.insert(share.clone(), stored.layout_id.clone());
        Ok(share)
    }
    /// The current version of a shared original; never owner material.
    pub fn shared(&self, share_id: &str) -> Result<LayoutView, LayoutError> {
        let index = self.index.lock().map_err(|_| LayoutError::Internal)?;
        let layout_id = index.shares.get(share_id).ok_or(LayoutError::Unknown)?;
        let stored = self
            .load(layout_id)
            .map_err(|_| LayoutError::Internal)?
            .ok_or(LayoutError::Unknown)?;
        self.view(&stored)
    }
    /// A recipient's independent original, starting from the share's current version.
    pub fn copy(&self, share_id: &str, name: &str) -> Result<Created, LayoutError> {
        let source = self.shared(share_id)?;
        self.create(name, source.layout)
    }
    /// Resolves a retained saved version for match setup. Unknown, forged, dropped or
    /// deleted identities are `None`.
    pub fn board(&self, identity: &ContentIdentity) -> Option<BoardProfile> {
        let index = self.index.lock().ok()?;
        let layout_id = index.boards.get(&identity.name)?;
        let stored = self.load(layout_id).ok()??;
        stored
            .versions
            .iter()
            .filter(|version| version.source.name == identity.name)
            .filter_map(|version| board_profile(&version.source, &version.layout).ok())
            .find(|profile| profile.identity() == identity)
    }
}

pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
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
