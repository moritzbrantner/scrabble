//! Durable guest-owned custom word lists (#97; Decisions 1.1, 1.8, 1.16, 1.18, 1.20,
//! 1.25, 1.45, 1.48, 1.50, 1.52).
//!
//! Mirrors the custom board-layout library (#101). Each original is one private JSON
//! entry holding a keyed verifier of its owner capability (never the capability), its
//! current name, an optional share id and the metadata of its recent immutable
//! versions. Each retained version's normalized word set (sorted distinct words, one per
//! line, exactly the bytes the configuration store pins for a match) lives in its own
//! private content file and is verified by SHA-256 whenever it is read.
//!
//! Word text is parsed by the existing dictionary parser (`WordList::from_text`) and
//! fingerprinted by `WordListProfile`, so no second parser or identity scheme exists.
//! Started matches never read this library again: their configured initialization
//! manifest and content-addressed word store (ADR 0009) pin the selected word set.
use crate::{
    ErrorResult,
    dictionary::hexadecimal,
    layout_library::{decode_hex, is_hex_id, random_hex, sync_directory},
    profiles::WordListProfile,
};
use ring::{digest, hmac};
use scrabble_game::{
    dictionary::{WordList, WordListError},
    ruleset::ContentIdentity,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime},
};

/// Operator-configurable bounds (Decision 1.50), published by `GET /word-lists/limits`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WordListLimits {
    /// UTF-8 bytes of one submitted word-list text.
    pub max_bytes: usize,
    /// Distinct words in one list.
    pub max_words: usize,
    /// Originals and copies kept by one server; protects disk use of anonymous guests.
    pub max_lists: usize,
    /// Recent immutable versions of one original that remain selectable for new matches.
    pub max_versions: usize,
    /// Total retained normalized word content across the whole library.
    pub max_stored_bytes: u64,
}
impl Default for WordListLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024,
            max_words: 200_000,
            max_lists: 1024,
            max_versions: 8,
            max_stored_bytes: 256 * 1024 * 1024,
        }
    }
}
/// Hard ceilings of the operator settings (`SCRABBLE_WORD_LIST_MAX_*`).
pub const CEILING_BYTES: u64 = 64 * 1024 * 1024;
pub const CEILING_WORDS: u64 = 10_000_000;
pub const CEILING_LISTS: u64 = 1_000_000;
pub const CEILING_VERSIONS: u64 = 64;
pub const CEILING_STORED_BYTES: u64 = 1 << 40;
const MAX_NAME_CHARS: usize = 64;
const MAX_ENTRY_BYTES: u64 = 64 * 1024;
/// Interrupted writes older than this are removed when the library is opened.
const ORPHAN_AGE: Duration = Duration::from_secs(600);

#[derive(Debug)]
pub enum WordListLibraryError {
    /// A well-formed but invalid list or name; `line` names the offending 1-based line.
    Invalid {
        reason: String,
        line: Option<usize>,
    },
    TooLarge(String),
    OwnerRejected,
    Unknown,
    Full,
    Internal,
}
type Result<T> = std::result::Result<T, WordListLibraryError>;
impl From<io::Error> for WordListLibraryError {
    fn from(_: io::Error) -> Self {
        Self::Internal
    }
}
impl From<crate::layout_library::LayoutError> for WordListLibraryError {
    fn from(_: crate::layout_library::LayoutError) -> Self {
        Self::Internal
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredVersion {
    revision: u64,
    /// Random per-version source identity; the public identity adds the content hash.
    source: ContentIdentity,
    word_list: ContentIdentity,
    content_hash: String,
    word_count: usize,
    bytes: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredWordList {
    version: u8,
    word_list_id: String,
    verifier: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    share_id: Option<String>,
    versions: Vec<StoredVersion>,
}
impl StoredWordList {
    fn current(&self) -> &StoredVersion {
        self.versions
            .last()
            .expect("a stored word list has a version")
    }
}

/// The current version of an original as returned to clients.
pub struct WordListView {
    pub word_list_id: String,
    pub name: String,
    pub revision: u64,
    pub word_list: ContentIdentity,
    pub word_count: usize,
    pub share_id: Option<String>,
}
pub struct Created {
    pub view: WordListView,
    pub owner_capability: String,
}

/// Proof of access for selecting a guest list in a NEW match (Decision 1.48): a live
/// share of that list, or its owner capability.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WordListAccess {
    #[serde(rename_all = "camelCase")]
    Share { share_id: String },
    #[serde(rename_all = "camelCase")]
    Owner {
        word_list_id: String,
        owner_capability: String,
    },
}

#[derive(Default)]
struct Index {
    /// Source (and public identity) name of a retained version -> its original.
    versions: HashMap<String, String>,
    shares: HashMap<String, String>,
    lists: usize,
    bytes: u64,
}

pub struct WordListLibrary {
    directory: PathBuf,
    limits: WordListLimits,
    key: hmac::Key,
    index: Mutex<Index>,
}

pub fn is_word_list_id(value: &str) -> bool {
    is_hex_id(value, "w_")
}
pub fn is_share_id(value: &str) -> bool {
    is_hex_id(value, "s_")
}
fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A validated, fingerprinted version ready to store.
struct Prepared {
    source: ContentIdentity,
    profile: WordListProfile,
    content: std::sync::Arc<[u8]>,
    hash: String,
    word_count: usize,
}
fn validate_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed != name
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(WordListLibraryError::Invalid {
            reason: format!(
                "word list name must be 1 to {MAX_NAME_CHARS} characters without surrounding spaces"
            ),
            line: None,
        });
    }
    Ok(())
}
/// Uppercases each lowercase letter with exactly one uppercase form (Decision 1.52).
/// A letter without one (`ß` -> `SS`, `ﬁ` -> `FI`) is replaced by a character the
/// dictionary parser rejects, so the parser still names the first faulty line; that
/// line is returned so the reason can name the cause.
fn uppercase(words: &str) -> (String, Option<usize>) {
    let mut text = String::with_capacity(words.len());
    let mut unmapped = None;
    for (index, line) in words.lines().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        for letter in line.chars() {
            if !letter.is_lowercase() {
                text.push(letter);
                continue;
            }
            let mut upper = letter.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(single), None) if !single.is_lowercase() => text.push(single),
                _ => {
                    unmapped.get_or_insert(index + 1);
                    text.push('0');
                }
            }
        }
    }
    (text, unmapped)
}
/// Parses with the deployment dictionary parser and bounds the result.
fn prepare(source: ContentIdentity, words: &str, limits: &WordListLimits) -> Result<Prepared> {
    if words.len() > limits.max_bytes {
        return Err(WordListLibraryError::TooLarge(format!(
            "word lists are limited to {} bytes of UTF-8 text",
            limits.max_bytes
        )));
    }
    let (normalized, unmapped) = uppercase(words);
    let list = WordList::from_text(source.clone(), &normalized).map_err(|error| match error {
        WordListError::InvalidWord { line } if unmapped == Some(line) => {
            WordListLibraryError::Invalid {
                reason: format!(
                    "line {line} contains a lowercase letter without a single uppercase form (such as ß); write it in uppercase"
                ),
                line: Some(line),
            }
        }
        WordListError::InvalidWord { line } => WordListLibraryError::Invalid {
            reason: format!(
                "line {line} must be one word of letters only, without spaces, digits, punctuation or comments"
            ),
            line: Some(line),
        },
        WordListError::Empty => WordListLibraryError::Invalid {
            reason: "the word list contains no words".into(),
            line: None,
        },
        WordListError::InvalidIdentity => WordListLibraryError::Internal,
    })?;
    let word_count = list.words().count();
    if word_count > limits.max_words {
        return Err(WordListLibraryError::TooLarge(format!(
            "word lists are limited to {} distinct words",
            limits.max_words
        )));
    }
    let profile = WordListProfile::new(list).map_err(|_| WordListLibraryError::Internal)?;
    let (content, hash) = profile.content();
    Ok(Prepared {
        source,
        profile,
        content,
        hash,
        word_count,
    })
}
fn new_source(revision: u64) -> Result<ContentIdentity> {
    Ok(ContentIdentity {
        name: format!("custom-words-{}", random_hex(16)?),
        revision: revision.to_string(),
    })
}
fn write_private(
    directory: &std::path::Path,
    target: &std::path::Path,
    bytes: &[u8],
) -> Result<()> {
    let temporary = directory.join(format!(".{}.tmp", random_hex(16)?));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> io::Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, target)?;
        sync_directory(directory)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    Ok(result?)
}

impl WordListLibrary {
    /// Opens (creating if needed) the private library directory and indexes it.
    pub fn open(
        directory: PathBuf,
        derivation: &hmac::Key,
        limits: WordListLimits,
    ) -> ErrorResult<Self> {
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let key = hmac::Key::new(
            hmac::HMAC_SHA256,
            hmac::sign(derivation, b"scrabble/word-list-owner/v1\0").as_ref(),
        );
        let library = Self {
            directory,
            limits,
            key,
            index: Mutex::new(Index::default()),
        };
        let mut index = Index::default();
        let mut retained = HashSet::new();
        let mut others = Vec::new();
        for entry in fs::read_dir(&library.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(text) = name.to_str() else {
                continue;
            };
            match text.strip_suffix(".json").filter(|id| is_word_list_id(id)) {
                Some(id) => {
                    let stored = library
                        .load(id)?
                        .ok_or("word list disappeared while indexing")?;
                    for version in &stored.versions {
                        retained.insert(Self::content_name(id, version.revision));
                    }
                    Self::insert(&mut index, &stored);
                }
                None => others.push(entry.path()),
            }
        }
        // Content of a crashed or superseded write is never selectable; remove it once it
        // is old enough not to belong to a concurrent writer of the same directory.
        let now = SystemTime::now();
        for path in others {
            let file = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let orphan = (file.ends_with(".txt") && !retained.contains(file))
                || (file.starts_with('.') && file.ends_with(".tmp"));
            let old = fs::symlink_metadata(&path)
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| {
                    now.duration_since(modified)
                        .is_ok_and(|age| age > ORPHAN_AGE)
                });
            if orphan && old {
                let _ = fs::remove_file(&path);
            }
        }
        *library
            .index
            .lock()
            .map_err(|_| "word list index unavailable")? = index;
        Ok(library)
    }
    fn insert(index: &mut Index, stored: &StoredWordList) {
        for version in &stored.versions {
            index
                .versions
                .insert(version.source.name.clone(), stored.word_list_id.clone());
            index.bytes += version.bytes;
        }
        if let Some(share) = &stored.share_id {
            index
                .shares
                .insert(share.clone(), stored.word_list_id.clone());
        }
        index.lists += 1;
    }
    fn content_name(id: &str, revision: u64) -> String {
        format!("{id}-r{revision}.txt")
    }
    fn path(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }
    fn load(&self, id: &str) -> ErrorResult<Option<StoredWordList>> {
        let path = self.path(id);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() {
            return Err("word list entries must be regular files".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ENTRY_BYTES {
            return Err("word list entry exceeds its supported size".into());
        }
        let stored: StoredWordList = serde_json::from_slice(&bytes)?;
        if stored.version != 1
            || stored.word_list_id != id
            || stored.versions.is_empty()
            || stored.versions.len() as u64 > CEILING_VERSIONS
            || stored.versions.iter().any(|v| !is_hash(&v.content_hash))
            || stored.share_id.as_deref().is_some_and(|s| !is_share_id(s))
        {
            return Err("invalid persisted word list entry".into());
        }
        Ok(Some(stored))
    }
    fn store(&self, stored: &StoredWordList) -> Result<()> {
        let bytes = serde_json::to_vec(stored).map_err(|_| WordListLibraryError::Internal)?;
        write_private(&self.directory, &self.path(&stored.word_list_id), &bytes)
    }
    /// Verified normalized words of a retained version.
    fn content(&self, id: &str, version: &StoredVersion) -> Result<Vec<u8>> {
        let path = self
            .directory
            .join(Self::content_name(id, version.revision));
        if !fs::symlink_metadata(&path)?.file_type().is_file() {
            return Err(WordListLibraryError::Internal);
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)?
            .take(self.max_content_bytes() + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > self.max_content_bytes()
            || hexadecimal(digest::digest(&digest::SHA256, &bytes).as_ref()) != version.content_hash
        {
            return Err(WordListLibraryError::Internal);
        }
        Ok(bytes)
    }
    fn remove_content(&self, id: &str, versions: &[StoredVersion]) {
        for version in versions {
            let _ = fs::remove_file(
                self.directory
                    .join(Self::content_name(id, version.revision)),
            );
        }
        let _ = sync_directory(&self.directory);
    }
    fn owner_message(id: &str, capability: &str) -> Vec<u8> {
        let mut message = Vec::with_capacity(id.len() + 1 + capability.len());
        message.extend_from_slice(id.as_bytes());
        message.push(0);
        message.extend_from_slice(capability.as_bytes());
        message
    }
    /// Loads an original for its owner; the capability check is constant-time.
    fn owned(&self, id: &str, capability: &str) -> Result<StoredWordList> {
        if !is_word_list_id(id) {
            return Err(WordListLibraryError::Unknown);
        }
        let stored = self
            .load(id)
            .map_err(|_| WordListLibraryError::Internal)?
            .ok_or(WordListLibraryError::Unknown)?;
        let expected = decode_hex(&stored.verifier).ok_or(WordListLibraryError::Internal)?;
        // `hmac::verify` recomputes the tag and compares it in constant time.
        hmac::verify(&self.key, &Self::owner_message(id, capability), &expected)
            .map_err(|_| WordListLibraryError::OwnerRejected)?;
        Ok(stored)
    }
    fn view(stored: &StoredWordList) -> WordListView {
        let current = stored.current();
        WordListView {
            word_list_id: stored.word_list_id.clone(),
            name: stored.name.clone(),
            revision: current.revision,
            word_list: current.word_list.clone(),
            word_count: current.word_count,
            share_id: stored.share_id.clone(),
        }
    }
    fn version(revision: u64, prepared: &Prepared) -> StoredVersion {
        StoredVersion {
            revision,
            source: prepared.source.clone(),
            word_list: prepared.profile.identity().clone(),
            content_hash: prepared.hash.clone(),
            word_count: prepared.word_count,
            bytes: prepared.content.len() as u64,
        }
    }
    fn write_content(&self, id: &str, revision: u64, prepared: &Prepared) -> Result<()> {
        write_private(
            &self.directory,
            &self.directory.join(Self::content_name(id, revision)),
            &prepared.content,
        )
    }

    pub fn create(&self, name: &str, words: &str) -> Result<Created> {
        validate_name(name)?;
        let prepared = prepare(new_source(1)?, words, &self.limits)?;
        let mut index = self
            .index
            .lock()
            .map_err(|_| WordListLibraryError::Internal)?;
        let bytes = prepared.content.len() as u64;
        if index.lists >= self.limits.max_lists
            || index.bytes + bytes > self.limits.max_stored_bytes
        {
            return Err(WordListLibraryError::Full);
        }
        let id = format!("w_{}", random_hex(16)?);
        let capability = random_hex(32)?;
        let stored = StoredWordList {
            version: 1,
            verifier: hexadecimal(
                hmac::sign(&self.key, &Self::owner_message(&id, &capability)).as_ref(),
            ),
            word_list_id: id.clone(),
            name: name.to_owned(),
            share_id: None,
            versions: vec![Self::version(1, &prepared)],
        };
        self.write_content(&id, 1, &prepared)?;
        if let Err(error) = self.store(&stored) {
            self.remove_content(&id, &stored.versions);
            return Err(error);
        }
        Self::insert(&mut index, &stored);
        Ok(Created {
            view: Self::view(&stored),
            owner_capability: capability,
        })
    }
    /// The owner's view plus its saved words, one per line.
    pub fn read(&self, id: &str, capability: &str) -> Result<(WordListView, String)> {
        let _index = self
            .index
            .lock()
            .map_err(|_| WordListLibraryError::Internal)?;
        let stored = self.owned(id, capability)?;
        let words = self.content(id, stored.current())?;
        Ok((
            Self::view(&stored),
            String::from_utf8(words).map_err(|_| WordListLibraryError::Internal)?,
        ))
    }
    /// Saves a new immutable version; earlier versions stay pinned by started matches.
    pub fn update(
        &self,
        id: &str,
        capability: &str,
        name: &str,
        words: &str,
    ) -> Result<(WordListView, String)> {
        let mut index = self
            .index
            .lock()
            .map_err(|_| WordListLibraryError::Internal)?;
        let mut stored = self.owned(id, capability)?;
        validate_name(name)?;
        let revision = stored.current().revision + 1;
        let prepared = prepare(new_source(revision)?, words, &self.limits)?;
        stored.name = name.to_owned();
        stored.versions.push(Self::version(revision, &prepared));
        let dropped: Vec<_> = if stored.versions.len() > self.limits.max_versions {
            let excess = stored.versions.len() - self.limits.max_versions;
            stored.versions.drain(..excess).collect()
        } else {
            Vec::new()
        };
        let released: u64 = dropped.iter().map(|version| version.bytes).sum();
        let added = prepared.content.len() as u64;
        if (index.bytes + added).saturating_sub(released) > self.limits.max_stored_bytes {
            return Err(WordListLibraryError::Full);
        }
        self.write_content(id, revision, &prepared)?;
        if let Err(error) = self.store(&stored) {
            self.remove_content(id, &stored.versions[stored.versions.len() - 1..]);
            return Err(error);
        }
        self.remove_content(id, &dropped);
        for version in &dropped {
            index.versions.remove(&version.source.name);
        }
        index
            .versions
            .insert(prepared.source.name.clone(), id.to_owned());
        index.bytes = (index.bytes + added).saturating_sub(released);
        let words = String::from_utf8(prepared.content.to_vec())
            .map_err(|_| WordListLibraryError::Internal)?;
        Ok((Self::view(&stored), words))
    }
    /// Removes the original from the library and ends its sharing. Started matches keep
    /// their pinned copies in the configuration store.
    pub fn delete(&self, id: &str, capability: &str) -> Result<()> {
        let mut index = self
            .index
            .lock()
            .map_err(|_| WordListLibraryError::Internal)?;
        let stored = self.owned(id, capability)?;
        fs::remove_file(self.path(id))?;
        sync_directory(&self.directory)?;
        self.remove_content(id, &stored.versions);
        for version in &stored.versions {
            index.versions.remove(&version.source.name);
            index.bytes = index.bytes.saturating_sub(version.bytes);
        }
        if let Some(share) = &stored.share_id {
            index.shares.remove(share);
        }
        index.lists = index.lists.saturating_sub(1);
        Ok(())
    }
    /// Enables (idempotently) the original's share link. After `unshare`, sharing again
    /// issues a fresh share id; a withdrawn id is never revived.
    pub fn share(&self, id: &str, capability: &str) -> Result<String> {
        let mut index = self
            .index
            .lock()
            .map_err(|_| WordListLibraryError::Internal)?;
        let mut stored = self.owned(id, capability)?;
        if let Some(share) = &stored.share_id {
            return Ok(share.clone());
        }
        let share = format!("s_{}", random_hex(16)?);
        stored.share_id = Some(share.clone());
        self.store(&stored)?;
        index.shares.insert(share.clone(), id.to_owned());
        Ok(share)
    }
    /// Withdraws the share link (idempotent). Existing copies and matches are unaffected.
    pub fn unshare(&self, id: &str, capability: &str) -> Result<()> {
        let mut index = self
            .index
            .lock()
            .map_err(|_| WordListLibraryError::Internal)?;
        let mut stored = self.owned(id, capability)?;
        let Some(share) = stored.share_id.take() else {
            return Ok(());
        };
        self.store(&stored)?;
        index.shares.remove(&share);
        Ok(())
    }
    fn shared_entry(&self, index: &Index, share_id: &str) -> Result<StoredWordList> {
        let id = index
            .shares
            .get(share_id)
            .ok_or(WordListLibraryError::Unknown)?;
        self.load(id)
            .map_err(|_| WordListLibraryError::Internal)?
            .filter(|stored| stored.share_id.as_deref() == Some(share_id))
            .ok_or(WordListLibraryError::Unknown)
    }
    /// The current version of a shared original; never owner material.
    pub fn shared(&self, share_id: &str) -> Result<WordListView> {
        let index = self
            .index
            .lock()
            .map_err(|_| WordListLibraryError::Internal)?;
        Ok(Self::view(&self.shared_entry(&index, share_id)?))
    }
    /// A recipient's independent original, starting from the share's current version.
    pub fn copy(&self, share_id: &str, name: &str) -> Result<Created> {
        validate_name(name)?;
        let words = {
            let index = self
                .index
                .lock()
                .map_err(|_| WordListLibraryError::Internal)?;
            let stored = self.shared_entry(&index, share_id)?;
            self.content(&stored.word_list_id, stored.current())?
        };
        let words = String::from_utf8(words).map_err(|_| WordListLibraryError::Internal)?;
        self.create(name, &words)
    }
    /// Resolves a retained saved version for a NEW match, given proof of access through
    /// a live share of that list or its owner capability (Decision 1.48). Unknown, forged,
    /// dropped, deleted, withdrawn or unproven selections are `None`.
    pub(crate) fn word_list(
        &self,
        identity: &ContentIdentity,
        access: &WordListAccess,
    ) -> Option<WordListProfile> {
        let (source, bytes) = {
            let index = self.index.lock().ok()?;
            let id = index.versions.get(&identity.name)?;
            let stored = match access {
                WordListAccess::Share { share_id } => {
                    if index.shares.get(share_id) != Some(id) {
                        return None;
                    }
                    let stored = self.load(id).ok()??;
                    if stored.share_id.as_deref() != Some(share_id.as_str()) {
                        return None;
                    }
                    stored
                }
                WordListAccess::Owner {
                    word_list_id,
                    owner_capability,
                } => {
                    if word_list_id != id {
                        return None;
                    }
                    self.owned(id, owner_capability).ok()?
                }
            };
            // Only the most recent configured versions remain selectable (Decision 1.50).
            let retained = stored
                .versions
                .len()
                .saturating_sub(self.limits.max_versions);
            let version = stored.versions[retained..]
                .iter()
                .find(|version| version.source.name == identity.name)
                .filter(|version| &version.word_list == identity)?;
            (version.source.clone(), self.content(id, version).ok()?)
        };
        let words = WordList::from_text(source, std::str::from_utf8(&bytes).ok()?).ok()?;
        WordListProfile::new(words)
            .ok()
            .filter(|profile| profile.identity() == identity)
    }
    pub fn limits(&self) -> &WordListLimits {
        &self.limits
    }
    /// Normalized content stays within the submitted bound, allowing for uppercase forms
    /// that need more UTF-8 bytes and a final line break.
    fn max_content_bytes(&self) -> u64 {
        2 * self.limits.max_bytes as u64 + 2
    }
}
