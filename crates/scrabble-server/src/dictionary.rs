//! Deployment-owned word lists are loaded once, fingerprinted, and shared immutably by matches.
use crate::config::DictionaryConfig;
use ring::digest;
use scrabble_game::{
    dictionary::{Dictionary, WordList, authored_fixture},
    ruleset::{ContentIdentity, Ruleset, english_fixture},
};
use std::{error::Error, fs::File, io::Read, sync::Arc};

const MAX_WORD_LIST_BYTES: u64 = 64 * 1024 * 1024;
fn hexadecimal(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub fn load(
    config: Option<&DictionaryConfig>,
) -> Result<(Ruleset, Arc<dyn Dictionary>), Box<dyn Error + Send + Sync>> {
    let mut rules = english_fixture();
    let Some(config) = config else {
        return Ok((rules, authored_fixture()));
    };
    let file = File::open(&config.file)?;
    if !file.metadata()?.is_file() {
        return Err("dictionary must be a regular UTF-8 word-list file".into());
    }
    let mut text = String::new();
    file.take(MAX_WORD_LIST_BYTES + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_WORD_LIST_BYTES {
        return Err("dictionary exceeds 64 MiB limit".into());
    }
    let words = WordList::from_text(
        ContentIdentity {
            name: config.name.clone(),
            revision: config.revision.clone(),
        },
        &text,
    )?;
    let mut hash = digest::Context::new(&digest::SHA256);
    for word in words.words() {
        hash.update(word.as_bytes());
        hash.update(b"\n");
    }
    let words = words.with_revision(format!(
        "{}+sha256:{}",
        config.revision,
        hexadecimal(hash.finish().as_ref())
    ))?;
    rules.dictionary = words.identity().clone();
    let identity = serde_json::to_vec(&rules.dictionary)?;
    rules.identity.revision = format!(
        "1+dictionary-sha256:{}",
        hexadecimal(digest::digest(&digest::SHA256, &identity).as_ref())
    );
    rules.validate()?;
    Ok((rules, Arc::new(words)))
}
