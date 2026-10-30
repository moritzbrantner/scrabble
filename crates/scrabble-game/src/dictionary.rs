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
    TooLarge,
}
impl fmt::Display for WordListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for WordListError {}

/// Sorted unique words packed into one buffer (#216): multi-million-word presets would
/// otherwise cost one heap allocation and tree slot per word.
struct PackedWords {
    text: Box<str>,
    /// `starts[i]..starts[i + 1]` is word `i`; one more entry than there are words.
    starts: Box<[u32]>,
}
impl PackedWords {
    fn get(&self, index: usize) -> &str {
        &self.text[self.starts[index] as usize..self.starts[index + 1] as usize]
    }
    fn len(&self) -> usize {
        self.starts.len() - 1
    }
    fn contains(&self, word: &str) -> bool {
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let middle = low + (high - low) / 2;
            match self.get(middle).cmp(word) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }
}

/// Owns a validated word set; neither source order nor duplicate entries affect lookup.
#[derive(Clone)]
pub struct WordList {
    identity: ContentIdentity,
    words: Arc<PackedWords>,
}
impl WordList {
    pub fn from_text(identity: ContentIdentity, text: &str) -> Result<Self, WordListError> {
        if identity.name.is_empty() || identity.revision.is_empty() {
            return Err(WordListError::InvalidIdentity);
        }
        let mut words = Vec::new();
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
            words.push(word);
        }
        if words.is_empty() {
            return Err(WordListError::Empty);
        }
        words.sort_unstable();
        words.dedup();
        let mut packed = String::with_capacity(words.iter().map(|word| word.len()).sum());
        let mut starts = Vec::with_capacity(words.len() + 1);
        starts.push(0);
        for word in words {
            packed.push_str(word);
            starts.push(u32::try_from(packed.len()).map_err(|_| WordListError::TooLarge)?);
        }
        Ok(Self {
            identity,
            words: Arc::new(PackedWords {
                text: packed.into_boxed_str(),
                starts: starts.into_boxed_slice(),
            }),
        })
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
        (0..self.words.len()).map(|index| self.words.get(index))
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
