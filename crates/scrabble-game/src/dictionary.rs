//! The authored dictionary installed with the current fixture ruleset.
use crate::placement::ValidatedMove;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DictionaryError {
    IdentityMismatch,
    UnknownWord(String),
}
impl fmt::Display for DictionaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for DictionaryError {}

/// Fail closed for uninstalled content; deployment dictionaries follow in #24.
pub fn validate(validated: &ValidatedMove<'_>) -> Result<(), DictionaryError> {
    let identity = &validated.ruleset().dictionary;
    if identity.name != "scrabble-authored-fixture" || identity.revision != "1" {
        return Err(DictionaryError::IdentityMismatch);
    }
    for word in validated.words() {
        let text = word.text();
        if !include_str!("../fixtures/words.txt")
            .lines()
            .any(|entry| entry == text)
        {
            return Err(DictionaryError::UnknownWord(text));
        }
    }
    Ok(())
}
