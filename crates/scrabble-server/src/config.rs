//! One validated configuration boundary, independent of ambient environment in tests.
use game_server::{BrowserOriginAllowlist, BrowserRoutePrefix, MatchId};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
    time::Duration,
};

pub enum Mode {
    Local,
    Production,
}
pub struct DictionaryConfig {
    pub file: PathBuf,
    pub name: String,
    pub revision: String,
}
pub struct ServerConfig {
    pub mode: Mode,
    pub dictionary: Option<DictionaryConfig>,
    pub ruleset_file: Option<PathBuf>,
    pub profile_catalog_file: Option<PathBuf>,
    pub match_configuration_directory: PathBuf,
    /// Durable private guest board-layout library (#101).
    pub board_layout_directory: PathBuf,
    /// Durable private guest word-list library (#97).
    pub word_list_directory: PathBuf,
    /// Operator-configurable guest word-list bounds (#97, Decision 1.50).
    pub word_list_limits: crate::word_list_library::WordListLimits,
    pub port: u16,
    pub status_port: u16,
    pub api_port: u16,
    pub board_origin: String,
    pub allowed_origins: Option<BrowserOriginAllowlist>,
    pub certificate: PathBuf,
    pub private_key: PathBuf,
    pub seed_file: PathBuf,
    pub recovery_directory: PathBuf,
    pub route_prefix: BrowserRoutePrefix,
    pub match_ids: Vec<MatchId>,
    pub max_matches: usize,
    pub reconnect_grace_ticks: u64,
    pub drain_grace: Duration,
    pub(crate) test_seed: Option<[u8; 32]>,
}
#[derive(Debug)]
pub struct ConfigError(String);
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ConfigError {}
fn fail(message: impl Into<String>) -> ConfigError {
    ConfigError(message.into())
}
const KEYS: &[&str] = &[
    "SCRABBLE_PROFILE_CATALOG_FILE",
    "SCRABBLE_MATCH_CONFIGURATION_DIR",
    "SCRABBLE_BOARD_LAYOUT_DIR",
    "SCRABBLE_WORD_LIST_DIR",
    "SCRABBLE_WORD_LIST_MAX_BYTES",
    "SCRABBLE_WORD_LIST_MAX_WORDS",
    "SCRABBLE_WORD_LIST_MAX_LISTS",
    "SCRABBLE_WORD_LIST_MAX_VERSIONS",
    "SCRABBLE_WORD_LIST_MAX_STORED_BYTES",
    "SCRABBLE_MODE",
    "SCRABBLE_RULESET_FILE",
    "SCRABBLE_DICTIONARY_FILE",
    "SCRABBLE_DICTIONARY_NAME",
    "SCRABBLE_DICTIONARY_REVISION",
    "SCRABBLE_PORT",
    "SCRABBLE_STATUS_PORT",
    "SCRABBLE_API_PORT",
    "SCRABBLE_BOARD_ORIGIN",
    "SCRABBLE_CERT_PEM",
    "SCRABBLE_KEY_PEM",
    "SCRABBLE_SEED_FILE",
    "SCRABBLE_RECOVERY_DIR",
    "SCRABBLE_ROUTE_PREFIX",
    "SCRABBLE_MATCH_IDS",
    "SCRABBLE_MAX_MATCHES",
    "SCRABBLE_RECONNECT_GRACE_TICKS",
    "SCRABBLE_DRAIN_GRACE_MS",
    "SCRABBLE_TEST_SEED",
];
fn value<'a>(values: &'a BTreeMap<String, String>, key: &str, default: &'a str) -> &'a str {
    values.get(key).map(String::as_str).unwrap_or(default)
}
fn number(
    values: &BTreeMap<String, String>,
    key: &str,
    default: &str,
    minimum: u64,
    maximum: u64,
) -> Result<u64, ConfigError> {
    let text = value(values, key, default);
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(fail(format!("{key} must be a decimal integer")));
    }
    let number: u64 = text
        .parse()
        .map_err(|_| fail(format!("{key} is outside its supported range")))?;
    if number < minimum || number > maximum {
        return Err(fail(format!(
            "{key} must be between {minimum} and {maximum}"
        )));
    }
    Ok(number)
}
fn path(
    values: &BTreeMap<String, String>,
    key: &str,
    default: &str,
) -> Result<PathBuf, ConfigError> {
    let text = value(values, key, default);
    if text.trim().is_empty() {
        return Err(fail(format!("{key} must name a path")));
    }
    Ok(PathBuf::from(text))
}
impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();
        for (key, value) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("SCRABBLE_") {
                let key = key
                    .into_string()
                    .map_err(|_| fail("non-UTF-8 server configuration name"))?;
                let value = value
                    .into_string()
                    .map_err(|_| fail(format!("{key} must be UTF-8")))?;
                values.insert(key, value);
            }
        }
        Self::from_values(&values)
    }
    pub fn from_values(values: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        for key in values.keys() {
            if !KEYS.contains(&key.as_str()) {
                return Err(fail(format!("unknown server configuration key {key}")));
            }
        }
        let mode = match value(values, "SCRABBLE_MODE", "local") {
            "local" => Mode::Local,
            "production" => Mode::Production,
            _ => return Err(fail("SCRABBLE_MODE must be local or production")),
        };
        if matches!(mode, Mode::Production) {
            for key in [
                "SCRABBLE_CERT_PEM",
                "SCRABBLE_KEY_PEM",
                "SCRABBLE_SEED_FILE",
                "SCRABBLE_RECOVERY_DIR",
                "SCRABBLE_MATCH_IDS",
            ] {
                if !values.contains_key(key) {
                    return Err(fail(format!("production requires explicit {key}")));
                }
            }
            if values.contains_key("SCRABBLE_TEST_SEED") {
                return Err(fail("SCRABBLE_TEST_SEED is forbidden in production"));
            }
        }
        let port = number(values, "SCRABBLE_PORT", "4433", 1, 65535)? as u16;
        let status_port = number(values, "SCRABBLE_STATUS_PORT", "8080", 1, 65535)? as u16;
        let api_port = number(values, "SCRABBLE_API_PORT", "8081", 1, 65535)? as u16;
        if api_port == status_port {
            return Err(fail("API and status listeners require different TCP ports"));
        }
        let board_origin = value(values, "SCRABBLE_BOARD_ORIGIN", "http://localhost:5173");
        let origin = url::Url::parse(board_origin)
            .map_err(|_| fail("SCRABBLE_BOARD_ORIGIN must be a canonical HTTP(S) origin"))?;
        if !matches!(origin.scheme(), "http" | "https")
            || origin.origin().ascii_serialization() != board_origin
        {
            return Err(fail(
                "SCRABBLE_BOARD_ORIGIN must be a canonical HTTP(S) origin without a path",
            ));
        }
        if matches!(mode, Mode::Production)
            && (!values.contains_key("SCRABBLE_BOARD_ORIGIN") || origin.scheme() != "https")
        {
            return Err(fail(
                "production requires an explicit HTTPS SCRABBLE_BOARD_ORIGIN",
            ));
        }
        let max_matches = number(values, "SCRABBLE_MAX_MATCHES", "16", 1, 64)? as usize;
        let allowed_origins = if matches!(mode, Mode::Production) {
            Some(BrowserOriginAllowlist::new([board_origin]).map_err(|_| {
                fail("production board origin exceeds the supported browser-origin bound")
            })?)
        } else {
            None
        };
        let ids = value(values, "SCRABBLE_MATCH_IDS", "table-1,table-2");
        let mut unique = BTreeSet::new();
        let mut match_ids = Vec::new();
        for id in ids.split(',').filter(|_| !ids.is_empty()) {
            let id = MatchId::new(id).map_err(|_| {
                fail("SCRABBLE_MATCH_IDS must contain canonical comma-separated match IDs")
            })?;
            if !unique.insert(id.clone()) {
                return Err(fail("SCRABBLE_MATCH_IDS contains duplicates"));
            }
            match_ids.push(id);
        }
        if match_ids.len() > max_matches {
            return Err(fail("configured matches exceed SCRABBLE_MAX_MATCHES"));
        }
        match_ids.sort();
        let route_prefix = BrowserRoutePrefix::new(value(values, "SCRABBLE_ROUTE_PREFIX", "/game"))
            .map_err(|_| fail("SCRABBLE_ROUTE_PREFIX must be a canonical absolute base path"))?;
        let test_seed = values
            .get("SCRABBLE_TEST_SEED")
            .map(|text| {
                if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(fail("SCRABBLE_TEST_SEED must be 64 hexadecimal characters"));
                }
                let mut seed = [0; 32];
                for (index, byte) in seed.iter_mut().enumerate() {
                    *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                        .map_err(|_| fail("invalid test seed"))?;
                }
                Ok(seed)
            })
            .transpose()?;
        let dictionary_keys = [
            "SCRABBLE_DICTIONARY_FILE",
            "SCRABBLE_DICTIONARY_NAME",
            "SCRABBLE_DICTIONARY_REVISION",
        ];
        let dictionary = if dictionary_keys.iter().any(|key| values.contains_key(*key)) {
            if !dictionary_keys.iter().all(|key| values.contains_key(*key)) {
                return Err(fail(
                    "dictionary configuration requires FILE, NAME and REVISION together",
                ));
            }
            let name = value(values, "SCRABBLE_DICTIONARY_NAME", "");
            let revision = value(values, "SCRABBLE_DICTIONARY_REVISION", "");
            for text in [name, revision] {
                if text.is_empty()
                    || text.len() > 64
                    || !text
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                {
                    return Err(fail(
                        "dictionary name and revision must be 1 to 64 ASCII letters, digits, hyphens, underscores or dots",
                    ));
                }
            }
            let file = path(values, "SCRABBLE_DICTIONARY_FILE", "")?;
            if matches!(mode, Mode::Production) && !file.is_absolute() {
                return Err(fail("production dictionary path must be absolute"));
            }
            Some(DictionaryConfig {
                file,
                name: name.to_owned(),
                revision: revision.to_owned(),
            })
        } else {
            None
        };
        let ruleset_file = if values.contains_key("SCRABBLE_RULESET_FILE") {
            if dictionary.is_none() {
                return Err(fail(
                    "custom rulesets require explicit dictionary configuration",
                ));
            }
            let file = path(values, "SCRABBLE_RULESET_FILE", "")?;
            if matches!(mode, Mode::Production) && !file.is_absolute() {
                return Err(fail("production ruleset path must be absolute"));
            }
            Some(file)
        } else {
            None
        };
        let profile_catalog_file = values
            .get("SCRABBLE_PROFILE_CATALOG_FILE")
            .map(PathBuf::from);
        if profile_catalog_file.as_ref().is_some_and(|path| {
            path.as_os_str().is_empty() || (matches!(mode, Mode::Production) && !path.is_absolute())
        }) {
            return Err(fail(
                "profile catalog requires an explicit valid path, absolute in production",
            ));
        }
        let match_configuration_directory = match values.get("SCRABBLE_MATCH_CONFIGURATION_DIR") {
            Some(_) => path(values, "SCRABBLE_MATCH_CONFIGURATION_DIR", "")?,
            None => path(values, "SCRABBLE_SEED_FILE", ".local/server/seed")?
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("configurations"),
        };
        let board_layout_directory = match values.get("SCRABBLE_BOARD_LAYOUT_DIR") {
            Some(_) => path(values, "SCRABBLE_BOARD_LAYOUT_DIR", "")?,
            None => path(values, "SCRABBLE_SEED_FILE", ".local/server/seed")?
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("layouts"),
        };
        let word_list_directory = match values.get("SCRABBLE_WORD_LIST_DIR") {
            Some(_) => path(values, "SCRABBLE_WORD_LIST_DIR", "")?,
            None => path(values, "SCRABBLE_SEED_FILE", ".local/server/seed")?
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("word-lists"),
        };
        let word_list_limits = {
            use crate::word_list_library::{
                CEILING_BYTES, CEILING_LISTS, CEILING_STORED_BYTES, CEILING_VERSIONS,
                CEILING_WORDS, WordListLimits,
            };
            let defaults = WordListLimits::default();
            let setting = |key: &str, default: u64, maximum: u64| {
                number(values, key, &default.to_string(), 1, maximum)
            };
            WordListLimits {
                max_bytes: setting(
                    "SCRABBLE_WORD_LIST_MAX_BYTES",
                    defaults.max_bytes as u64,
                    CEILING_BYTES,
                )? as usize,
                max_words: setting(
                    "SCRABBLE_WORD_LIST_MAX_WORDS",
                    defaults.max_words as u64,
                    CEILING_WORDS,
                )? as usize,
                max_lists: setting(
                    "SCRABBLE_WORD_LIST_MAX_LISTS",
                    defaults.max_lists as u64,
                    CEILING_LISTS,
                )? as usize,
                max_versions: setting(
                    "SCRABBLE_WORD_LIST_MAX_VERSIONS",
                    defaults.max_versions as u64,
                    CEILING_VERSIONS,
                )? as usize,
                max_stored_bytes: setting(
                    "SCRABBLE_WORD_LIST_MAX_STORED_BYTES",
                    defaults.max_stored_bytes,
                    CEILING_STORED_BYTES,
                )?,
            }
        };
        let config = Self {
            board_layout_directory,
            word_list_directory,
            word_list_limits,
            profile_catalog_file,
            match_configuration_directory,
            mode,
            dictionary,
            ruleset_file,
            port,
            status_port,
            api_port,
            board_origin: board_origin.to_owned(),
            allowed_origins,
            match_ids,
            max_matches,
            route_prefix,
            test_seed,
            certificate: path(values, "SCRABBLE_CERT_PEM", ".local/server/cert.pem")?,
            private_key: path(values, "SCRABBLE_KEY_PEM", ".local/server/key.pem")?,
            seed_file: path(values, "SCRABBLE_SEED_FILE", ".local/server/seed")?,
            recovery_directory: path(values, "SCRABBLE_RECOVERY_DIR", ".local/server/recovery")?,
            reconnect_grace_ticks: number(
                values,
                "SCRABBLE_RECONNECT_GRACE_TICKS",
                "1200",
                1,
                72000,
            )?,
            drain_grace: Duration::from_millis(number(
                values,
                "SCRABBLE_DRAIN_GRACE_MS",
                "500",
                0,
                30000,
            )?),
        };
        if matches!(config.mode, Mode::Production)
            && [
                &config.certificate,
                &config.private_key,
                &config.seed_file,
                &config.recovery_directory,
                &config.match_configuration_directory,
                &config.board_layout_directory,
                &config.word_list_directory,
            ]
            .iter()
            .any(|path| !path.is_absolute())
        {
            return Err(fail(
                "production TLS, seed and recovery paths must be absolute",
            ));
        }
        if config.seed_file == config.certificate
            || config.seed_file == config.private_key
            || config.seed_file.starts_with(&config.recovery_directory)
        {
            return Err(fail(
                "seed file must be separate from TLS files and the consumed recovery directory",
            ));
        }
        if config.match_configuration_directory == config.seed_file
            || config.match_configuration_directory == config.certificate
            || config.match_configuration_directory == config.private_key
            || config
                .match_configuration_directory
                .starts_with(&config.recovery_directory)
            || config
                .recovery_directory
                .starts_with(&config.match_configuration_directory)
        {
            return Err(fail(
                "match initialization must be separate from TLS, seed and runtime recovery",
            ));
        }
        let layouts = &config.board_layout_directory;
        if layouts == &config.seed_file
            || layouts == &config.certificate
            || layouts == &config.private_key
            || layouts.starts_with(&config.recovery_directory)
            || config.recovery_directory.starts_with(layouts)
            || layouts.starts_with(&config.match_configuration_directory)
            || config.match_configuration_directory.starts_with(layouts)
        {
            return Err(fail(
                "board layout library must be separate from TLS, seed, recovery and match initialization",
            ));
        }
        let words = &config.word_list_directory;
        if words == &config.seed_file
            || words == &config.certificate
            || words == &config.private_key
            || words.starts_with(&config.recovery_directory)
            || config.recovery_directory.starts_with(words)
            || words.starts_with(&config.match_configuration_directory)
            || config.match_configuration_directory.starts_with(words)
            || words.starts_with(layouts)
            || layouts.starts_with(words)
        {
            return Err(fail(
                "word list library must be separate from TLS, seed, recovery, match initialization and board layouts",
            ));
        }
        if config.certificate == config.private_key {
            return Err(fail(
                "TLS certificate and private key require separate paths",
            ));
        }
        Ok(config)
    }
}
