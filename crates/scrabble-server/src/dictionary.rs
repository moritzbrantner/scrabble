//! Deployment-owned word lists are loaded once, fingerprinted, and shared immutably by matches.
use crate::config::DictionaryConfig;
use ring::digest;
use scrabble_game::{
    dictionary::{Dictionary, WordList, authored_fixture},
    ruleset::{ContentIdentity, Ruleset, english_fixture},
};
use std::{collections::BTreeSet, error::Error, fs::File, io::Read, path::Path, sync::Arc};

const MAX_WORD_LIST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RULESET_BYTES: u64 = 128 * 1024;
fn hexadecimal(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub fn load(
    ruleset_file: Option<&Path>,
    config: Option<&DictionaryConfig>,
) -> Result<(Ruleset, Arc<dyn Dictionary>), Box<dyn Error + Send + Sync>> {
    let mut rules = match ruleset_file {
        None => english_fixture(),
        Some(path) => {
            let file = File::open(path)?;
            if !file.metadata()?.is_file() {
                return Err("ruleset must be a regular JSON file".into());
            }
            let mut bytes = Vec::new();
            file.take(MAX_RULESET_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_RULESET_BYTES {
                return Err("ruleset exceeds 128 KiB limit".into());
            }
            let rules: Ruleset =
                serde_json::from_slice(&bytes).map_err(|_| "invalid ruleset JSON data")?;
            rules.validate()?;
            if rules.identity.revision.len() > 64 {
                return Err("configured ruleset base revision exceeds 64 bytes".into());
            }
            rules
        }
    };
    let Some(config) = config else {
        if ruleset_file.is_some() {
            return Err("custom rulesets require an explicit dictionary".into());
        }
        return Ok((rules, authored_fixture()));
    };
    if ruleset_file.is_some()
        && (rules.dictionary.name != config.name || rules.dictionary.revision != config.revision)
    {
        return Err("ruleset and configured dictionary identities differ".into());
    }
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
    let alphabet: BTreeSet<_> = rules
        .tiles
        .iter()
        .filter_map(|tile| match tile.face {
            scrabble_game::ruleset::TileFace::Letter { letter } => Some(letter),
            scrabble_game::ruleset::TileFace::Blank {} => None,
        })
        .collect();
    if !words
        .words()
        .all(|word| word.chars().all(|letter| alphabet.contains(&letter)))
    {
        return Err("dictionary contains letters outside the configured alphabet".into());
    }
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
    if ruleset_file.is_some() {
        // Serialization normalizes field order/whitespace while preserving semantic tile/premium order.
        let content = serde_json::to_vec(&rules)?;
        rules.identity.revision = format!(
            "{}+rules-sha256:{}",
            rules.identity.revision,
            hexadecimal(digest::digest(&digest::SHA256, &content).as_ref())
        );
    } else {
        let identity = serde_json::to_vec(&rules.dictionary)?;
        rules.identity.revision = format!(
            "1+dictionary-sha256:{}",
            hexadecimal(digest::digest(&digest::SHA256, &identity).as_ref())
        );
    }
    rules.validate()?;
    Ok((rules, Arc::new(words)))
}
