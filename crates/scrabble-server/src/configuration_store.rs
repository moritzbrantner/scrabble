//! Immutable initialization, separate from upstream canonical runtime recovery ownership.
use crate::{
    ErrorResult,
    profile_catalog::{PreparedConfiguration, Selection, read_bounded},
    profiles::{
        BoardProfile, ComposedRuleset, MatchRules, ProfileSelection, TileProfile, WordListProfile,
        compose_profiles,
    },
};
use game_server::MatchId;
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};
use scrabble_game::{dictionary::WordList, ruleset::Ruleset};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u8,
    selection: Selection,
    sources: ProfileSelection,
    rules: Ruleset,
    dictionary_hash: String,
    authentication: String,
}
pub(crate) struct RegisteredConfiguration {
    pub selection: Selection,
    pub configuration: ComposedRuleset,
}
pub(crate) struct ConfigurationStore {
    directory: PathBuf,
    key: hmac::Key,
    cache: Mutex<HashMap<String, Arc<RegisteredConfiguration>>>,
    restored_words: Mutex<HashMap<String, Arc<WordListProfile>>>,
}
fn regular_bytes(path: &Path, limit: u64) -> ErrorResult<Vec<u8>> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err("initialization input must be a regular file, not a symlink".into());
    }
    read_bounded(path, limit)
}
fn publish(path: &Path, bytes: &[u8]) -> ErrorResult<()> {
    let parent = path.parent().ok_or("initialization path needs a parent")?;
    fs::create_dir_all(parent)?;
    let mut random = [0; 16];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| "temporary input identity failed")?;
    let temporary = parent.join(format!(".{}.tmp", crate::dictionary::hexadecimal(&random)));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| -> ErrorResult<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        match fs::hard_link(&temporary, path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if regular_bytes(path, bytes.len() as u64)? != bytes {
                    return Err("immutable initialization input differs".into());
                }
            }
            Err(error) => return Err(error.into()),
        }
        #[cfg(unix)]
        {
            fs::File::open(parent)?.sync_all()?;
            if let Some(ancestor) = parent.parent().filter(|path| !path.as_os_str().is_empty()) {
                fs::File::open(ancestor)?.sync_all()?;
            }
        }
        Ok(())
    })();
    drop(file);
    let cleanup = fs::remove_file(&temporary);
    result?;
    cleanup?;
    Ok(())
}
impl ConfigurationStore {
    pub fn new(directory: PathBuf, key: hmac::Key) -> Self {
        Self {
            directory,
            key,
            cache: Mutex::new(HashMap::new()),
            restored_words: Mutex::new(HashMap::new()),
        }
    }
    fn authenticated_bytes(id: &MatchId, manifest: &Manifest) -> ErrorResult<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            "scrabble/configured-initialization/v1",
            id.as_str(),
            manifest.version,
            &manifest.selection,
            &manifest.sources,
            &manifest.rules,
            &manifest.dictionary_hash,
        ))?)
    }
    fn manifest(&self, id: &MatchId) -> PathBuf {
        self.directory.join(format!("{}.json", id.as_str()))
    }
    fn word_path(&self, hash: &str) -> PathBuf {
        self.directory.join("words").join(format!("{hash}.txt"))
    }
    fn word_bytes(&self, hash: &str) -> ErrorResult<Vec<u8>> {
        let bytes = regular_bytes(&self.word_path(hash), 64 * 1024 * 1024)?;
        if crate::dictionary::hexadecimal(digest::digest(&digest::SHA256, &bytes).as_ref()) != hash
        {
            return Err("persisted dictionary content fingerprint mismatch".into());
        }
        Ok(bytes)
    }
    pub fn get(&self, id: &MatchId) -> ErrorResult<Option<Arc<RegisteredConfiguration>>> {
        let path = self.manifest(id);
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if self
                    .cache
                    .lock()
                    .map_err(|_| "initialization cache unavailable")?
                    .contains_key(id.as_str())
                {
                    return Err("persisted match initialization is missing".into());
                }
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        }
        let manifest: Manifest = serde_json::from_slice(&regular_bytes(&path, 128 * 1024)?)?;
        if manifest.version != 1
            || manifest.dictionary_hash.len() != 64
            || !manifest
                .dictionary_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid persisted initialization".into());
        }
        let signature = (0..manifest.authentication.len())
            .step_by(2)
            .map(|index| {
                manifest
                    .authentication
                    .get(index..index + 2)
                    .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                    .ok_or("invalid initialization authenticator")
            })
            .collect::<Result<Vec<_>, _>>()?;
        hmac::verify(
            &self.key,
            &Self::authenticated_bytes(id, &manifest)?,
            &signature,
        )
        .map_err(|_| "initialization authentication failed")?;
        // Caches share parsed immutable values, never evidence that files remain durable.
        // Recheck both inputs before any successful retry or factory reconstruction.
        let bytes = self.word_bytes(&manifest.dictionary_hash)?;
        if let Some(value) = self
            .cache
            .lock()
            .map_err(|_| "initialization cache unavailable")?
            .get(id.as_str())
            .cloned()
        {
            return Ok(Some(value));
        }
        let key = serde_json::to_string(&(&manifest.sources.word_list, &manifest.dictionary_hash))?;
        let cached = self
            .restored_words
            .lock()
            .map_err(|_| "dictionary cache unavailable")?
            .get(&key)
            .cloned();
        let words = match cached {
            Some(value) => value,
            None => {
                let words = Arc::new(WordListProfile::new(WordList::from_text(
                    manifest.sources.word_list.clone(),
                    std::str::from_utf8(&bytes)?,
                )?)?);
                self.restored_words
                    .lock()
                    .map_err(|_| "dictionary cache unavailable")?
                    .insert(key, Arc::clone(&words));
                words
            }
        };
        let tiles = TileProfile::new(manifest.sources.tile, manifest.rules.tiles.clone())?;
        let board = BoardProfile::new(
            manifest.sources.board,
            manifest.rules.board_size,
            manifest.rules.premiums.clone(),
        )?;
        let composed = compose_profiles(&words, &tiles, &board, MatchRules::from(&manifest.rules))?;
        if composed.rules != manifest.rules
            || Selection::from(&composed.selection) != manifest.selection
        {
            return Err("persisted match configuration fingerprint mismatch".into());
        }
        let result = Arc::new(RegisteredConfiguration {
            selection: manifest.selection,
            configuration: composed,
        });
        self.cache
            .lock()
            .map_err(|_| "initialization cache unavailable")?
            .insert(id.as_str().into(), Arc::clone(&result));
        Ok(Some(result))
    }
    pub fn register(&self, id: &MatchId, prepared: PreparedConfiguration) -> ErrorResult<()> {
        let selection = Selection::from(&prepared.composed.selection);
        if let Some(existing) = self.get(id)? {
            if existing.selection != selection
                || existing.configuration.rules != prepared.composed.rules
            {
                return Err("conflicting match initialization".into());
            }
            return Ok(());
        }
        let words = self.word_path(&prepared.word_hash);
        match fs::symlink_metadata(&words) {
            // Publish a missing version from the prepared immutable catalog bytes. Existing
            // versions must verify; never overwrite a conflicting or corrupt input.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                publish(&words, &prepared.word_content)?;
            }
            Err(error) => return Err(error.into()),
            Ok(_) => {
                self.word_bytes(&prepared.word_hash)?;
            }
        }
        let mut manifest = Manifest {
            version: 1,
            selection: selection.clone(),
            sources: prepared.sources,
            rules: prepared.composed.rules.clone(),
            dictionary_hash: prepared.word_hash,
            authentication: String::new(),
        };
        manifest.authentication = crate::dictionary::hexadecimal(
            hmac::sign(&self.key, &Self::authenticated_bytes(id, &manifest)?).as_ref(),
        );
        publish(&self.manifest(id), &serde_json::to_vec(&manifest)?)?;
        self.cache
            .lock()
            .map_err(|_| "initialization cache unavailable")?
            .insert(
                id.as_str().into(),
                Arc::new(RegisteredConfiguration {
                    selection,
                    configuration: prepared.composed,
                }),
            );
        Ok(())
    }
    pub fn retire(&self, id: &MatchId) -> ErrorResult<()> {
        self.cache
            .lock()
            .map_err(|_| "initialization cache unavailable")?
            .remove(id.as_str());
        match fs::remove_file(self.manifest(id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
