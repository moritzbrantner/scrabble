//! One validated configuration boundary, independent of ambient environment in tests.
//!
//! [`RawConfig`] is the single list of accepted `SCRABBLE_*` keys. `validate` applies the
//! cross-field rules and derives the fixed private state layout below `SCRABBLE_STATE_DIR`.
use game_server::{BrowserOriginAllowlist, BrowserRoutePrefix, MatchId};
use serde::{Deserialize, de};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
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
    /// Root of all durable private server state; every child below is fixed.
    pub state_directory: PathBuf,
    /// `<state>/configurations`: immutable match initialization.
    pub match_configuration_directory: PathBuf,
    /// `<state>/layouts`: durable private guest board-layout library (#101).
    pub board_layout_directory: PathBuf,
    /// `<state>/word-lists`: durable private guest word-list library (#97).
    pub word_list_directory: PathBuf,
    /// `<state>/tile-profiles`: durable private guest tile-profile library (#100).
    pub tile_profile_directory: PathBuf,
    /// `<state>/push`: private VAPID key and turn-notification subscriptions (#158).
    pub push_directory: PathBuf,
    /// Local test servers only: permit plain-HTTP mock push services.
    pub push_insecure_test: bool,
    /// Operator-configurable guest word-list bounds (#97, Decision 1.50).
    pub word_list_limits: crate::word_list_library::WordListLimits,
    /// Shared Moenarch account service base URL (#132), e.g. `https://moenarch.com/accounts/`;
    /// account linking is off when unset.
    /// Plain HTTP is accepted only in local mode.
    pub accounts_url: Option<url::Url>,
    pub port: u16,
    pub status_port: u16,
    pub api_port: u16,
    pub board_origin: String,
    /// Extra guest-only board origins (#206), e.g. the GitHub Pages demo. They may use
    /// the game, library and WebTransport routes, but never the account routes.
    pub guest_board_origins: Vec<String>,
    pub allowed_origins: Option<BrowserOriginAllowlist>,
    pub certificate: PathBuf,
    pub private_key: PathBuf,
    /// `<state>/seed`: private 32-byte initialization key.
    pub seed_file: PathBuf,
    /// `<state>/recovery`: upstream-owned consumable recovery bundle.
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
impl de::Error for ConfigError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        fail(message.to_string())
    }
    fn unknown_field(field: &str, _expected: &'static [&'static str]) -> Self {
        fail(format!("unknown server configuration key {PREFIX}{field}"))
    }
}
fn fail(message: impl Into<String>) -> ConfigError {
    ConfigError(message.into())
}
const PREFIX: &str = "SCRABBLE_";
const MAX_GUEST_BOARD_ORIGINS: usize = 4;

/// Every accepted key, named without the `SCRABBLE_` prefix. Values stay text so
/// validation can name the full key in its errors.
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
struct RawConfig {
    mode: Option<String>,
    state_dir: Option<String>,
    cert_pem: Option<String>,
    key_pem: Option<String>,
    port: Option<String>,
    status_port: Option<String>,
    api_port: Option<String>,
    board_origin: Option<String>,
    guest_board_origins: Option<String>,
    accounts_url: Option<String>,
    route_prefix: Option<String>,
    match_ids: Option<String>,
    max_matches: Option<String>,
    reconnect_grace_ticks: Option<String>,
    drain_grace_ms: Option<String>,
    test_seed: Option<String>,
    ruleset_file: Option<String>,
    dictionary_file: Option<String>,
    dictionary_name: Option<String>,
    dictionary_revision: Option<String>,
    profile_catalog_file: Option<String>,
    push_insecure_test: Option<String>,
    word_list_max_bytes: Option<String>,
    word_list_max_words: Option<String>,
    word_list_max_lists: Option<String>,
    word_list_max_versions: Option<String>,
    word_list_max_stored_bytes: Option<String>,
}

/// One configuration value: text that is present.
struct Text(String);
impl<'de> de::IntoDeserializer<'de, ConfigError> for Text {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}
impl<'de> de::Deserializer<'de> for Text {
    type Error = ConfigError;
    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ConfigError> {
        visitor.visit_string(self.0)
    }
    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ConfigError> {
        visitor.visit_some(self)
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        unit unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier
        ignored_any
    }
}

fn number(
    key: &str,
    text: Option<&str>,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, ConfigError> {
    let Some(text) = text else {
        return Ok(default);
    };
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
fn path(key: &str, text: Option<&str>, default: &str) -> Result<PathBuf, ConfigError> {
    let text = text.unwrap_or(default);
    if text.trim().is_empty() {
        return Err(fail(format!("{key} must name a path")));
    }
    Ok(PathBuf::from(text))
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();
        for (key, value) in std::env::vars_os() {
            if key.to_string_lossy().starts_with(PREFIX) {
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
        let mut entries = Vec::with_capacity(values.len());
        for (key, value) in values {
            let field = key
                .strip_prefix(PREFIX)
                .ok_or_else(|| fail(format!("unknown server configuration key {key}")))?;
            entries.push((field.to_owned(), Text(value.clone())));
        }
        RawConfig::deserialize(de::value::MapDeserializer::new(entries.into_iter()))?.validate()
    }
}

impl RawConfig {
    fn validate(self) -> Result<ServerConfig, ConfigError> {
        let mode = match self.mode.as_deref().unwrap_or("local") {
            "local" => Mode::Local,
            "production" => Mode::Production,
            _ => return Err(fail("SCRABBLE_MODE must be local or production")),
        };
        let production = matches!(mode, Mode::Production);
        if production {
            for (key, value) in [
                ("SCRABBLE_CERT_PEM", &self.cert_pem),
                ("SCRABBLE_KEY_PEM", &self.key_pem),
                ("SCRABBLE_STATE_DIR", &self.state_dir),
                ("SCRABBLE_MATCH_IDS", &self.match_ids),
            ] {
                if value.is_none() {
                    return Err(fail(format!("production requires explicit {key}")));
                }
            }
            if self.test_seed.is_some() {
                return Err(fail("SCRABBLE_TEST_SEED is forbidden in production"));
            }
        }
        let port = number("SCRABBLE_PORT", self.port.as_deref(), 4433, 1, 65535)? as u16;
        let status_port = number(
            "SCRABBLE_STATUS_PORT",
            self.status_port.as_deref(),
            8080,
            1,
            65535,
        )? as u16;
        let api_port = number(
            "SCRABBLE_API_PORT",
            self.api_port.as_deref(),
            8081,
            1,
            65535,
        )? as u16;
        if api_port == status_port {
            return Err(fail("API and status listeners require different TCP ports"));
        }
        let board_origin = self
            .board_origin
            .as_deref()
            .unwrap_or("http://localhost:5173");
        let origin = url::Url::parse(board_origin)
            .map_err(|_| fail("SCRABBLE_BOARD_ORIGIN must be a canonical HTTP(S) origin"))?;
        if !matches!(origin.scheme(), "http" | "https")
            || origin.origin().ascii_serialization() != board_origin
        {
            return Err(fail(
                "SCRABBLE_BOARD_ORIGIN must be a canonical HTTP(S) origin without a path",
            ));
        }
        if production && (self.board_origin.is_none() || origin.scheme() != "https") {
            return Err(fail(
                "production requires an explicit HTTPS SCRABBLE_BOARD_ORIGIN",
            ));
        }
        let guests = self.guest_board_origins.as_deref().unwrap_or("");
        let mut guest_board_origins: Vec<String> = Vec::new();
        for guest in guests.split(',').filter(|_| !guests.is_empty()) {
            let canonical = url::Url::parse(guest).ok().is_some_and(|url| {
                url.origin().ascii_serialization() == guest
                    && match url.scheme() {
                        "https" => true,
                        "http" => !production,
                        _ => false,
                    }
            });
            if !canonical || guest == board_origin || guest_board_origins.iter().any(|g| g == guest)
            {
                return Err(fail(
                    "SCRABBLE_GUEST_BOARD_ORIGINS must list distinct canonical origins other than SCRABBLE_BOARD_ORIGIN (HTTPS in production)",
                ));
            }
            guest_board_origins.push(guest.to_owned());
        }
        if guest_board_origins.len() > MAX_GUEST_BOARD_ORIGINS {
            return Err(fail(format!(
                "SCRABBLE_GUEST_BOARD_ORIGINS lists at most {MAX_GUEST_BOARD_ORIGINS} origins"
            )));
        }
        let accounts_url = match self.accounts_url.as_deref() {
            None => None,
            Some(text) => {
                let parsed = url::Url::parse(text)
                    .ok()
                    .filter(|url| {
                        url.has_host()
                            && url.username().is_empty()
                            && url.password().is_none()
                            && url.query().is_none()
                            && url.fragment().is_none()
                            && match url.scheme() {
                                "https" => true,
                                "http" => !production,
                                _ => false,
                            }
                    })
                    .map(|mut url| {
                        // A base URL: routes resolve below it (`<url>v1/sessions/verify`).
                        if !url.path().ends_with('/') {
                            let path = format!("{}/", url.path());
                            url.set_path(&path);
                        }
                        url
                    });
                Some(parsed.ok_or_else(|| {
                    fail(
                        "SCRABBLE_ACCOUNTS_URL must be an absolute URL without credentials, query or fragment (HTTPS in production)",
                    )
                })?)
            }
        };
        let max_matches = number(
            "SCRABBLE_MAX_MATCHES",
            self.max_matches.as_deref(),
            16,
            1,
            64,
        )? as usize;
        let allowed_origins = if production {
            let origins =
                std::iter::once(board_origin).chain(guest_board_origins.iter().map(String::as_str));
            Some(BrowserOriginAllowlist::new(origins).map_err(|_| {
                fail("production board origins exceed the supported browser-origin bound")
            })?)
        } else {
            None
        };
        let ids = self.match_ids.as_deref().unwrap_or("table-1,table-2");
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
        let route_prefix = BrowserRoutePrefix::new(self.route_prefix.as_deref().unwrap_or("/game"))
            .map_err(|_| fail("SCRABBLE_ROUTE_PREFIX must be a canonical absolute base path"))?;
        let test_seed = self
            .test_seed
            .as_deref()
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
        let dictionary = match (
            self.dictionary_file.as_deref(),
            self.dictionary_name,
            self.dictionary_revision,
        ) {
            (None, None, None) => None,
            (Some(file), Some(name), Some(revision)) => {
                for text in [&name, &revision] {
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
                let file = path("SCRABBLE_DICTIONARY_FILE", Some(file), "")?;
                if production && !file.is_absolute() {
                    return Err(fail("production dictionary path must be absolute"));
                }
                Some(DictionaryConfig {
                    file,
                    name,
                    revision,
                })
            }
            _ => {
                return Err(fail(
                    "dictionary configuration requires FILE, NAME and REVISION together",
                ));
            }
        };
        let ruleset_file = match self.ruleset_file.as_deref() {
            None => None,
            Some(_) if dictionary.is_none() => {
                return Err(fail(
                    "custom rulesets require explicit dictionary configuration",
                ));
            }
            Some(text) => {
                let file = path("SCRABBLE_RULESET_FILE", Some(text), "")?;
                if production && !file.is_absolute() {
                    return Err(fail("production ruleset path must be absolute"));
                }
                Some(file)
            }
        };
        let profile_catalog_file = self.profile_catalog_file.map(PathBuf::from);
        if profile_catalog_file
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty() || (production && !path.is_absolute()))
        {
            return Err(fail(
                "profile catalog requires an explicit valid path, absolute in production",
            ));
        }
        let push_insecure_test = match self.push_insecure_test.as_deref().unwrap_or("0") {
            "0" => false,
            "1" if !production => true,
            "1" => {
                return Err(fail(
                    "SCRABBLE_PUSH_INSECURE_TEST is forbidden in production",
                ));
            }
            _ => return Err(fail("SCRABBLE_PUSH_INSECURE_TEST must be 0 or 1")),
        };
        let word_list_limits = {
            use crate::word_list_library::{
                CEILING_BYTES, CEILING_LISTS, CEILING_STORED_BYTES, CEILING_VERSIONS,
                CEILING_WORDS, WordListLimits,
            };
            let defaults = WordListLimits::default();
            let setting = |key: &str, text: &Option<String>, default: u64, maximum: u64| {
                number(key, text.as_deref(), default, 1, maximum)
            };
            WordListLimits {
                max_bytes: setting(
                    "SCRABBLE_WORD_LIST_MAX_BYTES",
                    &self.word_list_max_bytes,
                    defaults.max_bytes as u64,
                    CEILING_BYTES,
                )? as usize,
                max_words: setting(
                    "SCRABBLE_WORD_LIST_MAX_WORDS",
                    &self.word_list_max_words,
                    defaults.max_words as u64,
                    CEILING_WORDS,
                )? as usize,
                max_lists: setting(
                    "SCRABBLE_WORD_LIST_MAX_LISTS",
                    &self.word_list_max_lists,
                    defaults.max_lists as u64,
                    CEILING_LISTS,
                )? as usize,
                max_versions: setting(
                    "SCRABBLE_WORD_LIST_MAX_VERSIONS",
                    &self.word_list_max_versions,
                    defaults.max_versions as u64,
                    CEILING_VERSIONS,
                )? as usize,
                max_stored_bytes: setting(
                    "SCRABBLE_WORD_LIST_MAX_STORED_BYTES",
                    &self.word_list_max_stored_bytes,
                    defaults.max_stored_bytes,
                    CEILING_STORED_BYTES,
                )?,
            }
        };
        let state_directory = path(
            "SCRABBLE_STATE_DIR",
            self.state_dir.as_deref(),
            ".local/server",
        )?;
        let certificate = path(
            "SCRABBLE_CERT_PEM",
            self.cert_pem.as_deref(),
            ".local/server/cert.pem",
        )?;
        let private_key = path(
            "SCRABBLE_KEY_PEM",
            self.key_pem.as_deref(),
            ".local/server/key.pem",
        )?;
        if production
            && [&state_directory, &certificate, &private_key]
                .iter()
                .any(|path| !path.is_absolute())
        {
            return Err(fail(
                "production SCRABBLE_STATE_DIR and TLS paths must be absolute",
            ));
        }
        if certificate == private_key {
            return Err(fail(
                "TLS certificate and private key require separate paths",
            ));
        }
        let state = |child: &str| state_directory.join(child);
        let seed_file = state("seed");
        let state_directories = [
            "recovery",
            "configurations",
            "layouts",
            "word-lists",
            "tile-profiles",
            "push",
        ]
        .map(state);
        let inside_state = |file: &Path| {
            file == seed_file || state_directories.iter().any(|dir| file.starts_with(dir))
        };
        if inside_state(&certificate) || inside_state(&private_key) {
            return Err(fail(
                "TLS files must be separate from the seed and state directories",
            ));
        }
        let [
            recovery_directory,
            match_configuration_directory,
            board_layout_directory,
            word_list_directory,
            tile_profile_directory,
            push_directory,
        ] = state_directories;
        Ok(ServerConfig {
            mode,
            dictionary,
            ruleset_file,
            profile_catalog_file,
            state_directory,
            match_configuration_directory,
            board_layout_directory,
            word_list_directory,
            tile_profile_directory,
            push_directory,
            push_insecure_test,
            word_list_limits,
            accounts_url,
            port,
            status_port,
            api_port,
            board_origin: board_origin.to_owned(),
            guest_board_origins,
            allowed_origins,
            certificate,
            private_key,
            seed_file,
            recovery_directory,
            route_prefix,
            match_ids,
            max_matches,
            reconnect_grace_ticks: number(
                "SCRABBLE_RECONNECT_GRACE_TICKS",
                self.reconnect_grace_ticks.as_deref(),
                1200,
                1,
                72000,
            )?,
            drain_grace: Duration::from_millis(number(
                "SCRABBLE_DRAIN_GRACE_MS",
                self.drain_grace_ms.as_deref(),
                500,
                0,
                30000,
            )?),
            test_seed,
        })
    }
}
