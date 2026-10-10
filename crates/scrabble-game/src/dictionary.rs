//! Immutable, deterministic dictionary lookup after structural validation.
use crate::{placement::ValidatedMove, ruleset::ContentIdentity};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt,
    sync::{Arc, OnceLock},
};

/// Implementations must be immutable, deterministic and local; lookup performs no external I/O.
pub trait Dictionary: Send + Sync {
    fn identity(&self) -> &ContentIdentity;
    fn contains(&self, word: &str) -> bool;
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DictionaryError {
    IdentityMismatch,
    InvalidWords { words: Vec<String> },
}
impl fmt::Debug for DictionaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityMismatch => f.write_str("IdentityMismatch"),
            Self::InvalidWords { words } => f
                .debug_struct("InvalidWords")
                .field("word_count", &words.len())
                .finish_non_exhaustive(),
        }
    }
}
impl fmt::Display for DictionaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for DictionaryError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WordListError {
    InvalidIdentity,
    InvalidWord { line: usize },
    Empty,
}
impl fmt::Display for WordListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for WordListError {}

/// Owns a validated word set; neither source order nor duplicate entries affect lookup.
pub struct WordList {
    identity: ContentIdentity,
    words: BTreeSet<String>,
}
impl WordList {
    pub fn from_text(identity: ContentIdentity, text: &str) -> Result<Self, WordListError> {
        if identity.name.is_empty() || identity.revision.is_empty() {
            return Err(WordListError::InvalidIdentity);
        }
        let mut words = BTreeSet::new();
        for (index, word) in text.lines().enumerate() {
            if word.is_empty() {
                continue;
            }
            if !word
                .chars()
                .all(|letter| letter.is_alphabetic() && !letter.is_lowercase())
            {
                return Err(WordListError::InvalidWord { line: index + 1 });
            }
            words.insert(word.to_owned());
        }
        if words.is_empty() {
            return Err(WordListError::Empty);
        }
        Ok(Self { identity, words })
    }
    pub fn with_revision(mut self, revision: String) -> Result<Self, WordListError> {
        if revision.is_empty() {
            return Err(WordListError::InvalidIdentity);
        }
        self.identity.revision = revision;
        Ok(self)
    }
    /// Sorted unique content for a deployment-owned content fingerprint, never a browser DTO.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.words.iter().map(String::as_str)
    }
}
impl Dictionary for WordList {
    fn identity(&self) -> &ContentIdentity {
        &self.identity
    }
    fn contains(&self, word: &str) -> bool {
        self.words.contains(word)
    }
}

#[expect(
    clippy::expect_used,
    reason = "the compiled-in authored dictionary is committed content validated by tests"
)]
pub fn authored_fixture() -> Arc<WordList> {
    static FIXTURE: OnceLock<Arc<WordList>> = OnceLock::new();
    Arc::clone(FIXTURE.get_or_init(|| {
        Arc::new(
            WordList::from_text(
                ContentIdentity {
                    name: "scrabble-authored-fixture".into(),
                    revision: "1".into(),
                },
                include_str!("../fixtures/words.txt"),
            )
            .expect("committed authored dictionary is valid"),
        )
    }))
}

pub fn validate(
    validated: &ValidatedMove<'_>,
    dictionary: &dyn Dictionary,
) -> Result<(), DictionaryError> {
    if dictionary.identity() != &validated.ruleset().dictionary {
        return Err(DictionaryError::IdentityMismatch);
    }
    let words: BTreeSet<_> = validated.words().iter().map(|word| word.text()).collect();
    let invalid: Vec<_> = words
        .into_iter()
        .filter(|word| !dictionary.contains(word))
        .collect();
    if invalid.is_empty() {
        Ok(())
    } else {
        Err(DictionaryError::InvalidWords { words: invalid })
    }
}
