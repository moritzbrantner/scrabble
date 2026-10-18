//! Immutable profile composition; gameplay remains owned by scrabble-game.
use crate::dictionary::{fingerprint_word_list, hexadecimal};
use ring::digest;
use scrabble_game::{
    dictionary::{Dictionary, WordList},
    ruleset::{
        BoardShape, ContentIdentity, Premium, PremiumPolicy, Ruleset, TileDefinition,
        WordLengthMetric, validate_board, validate_hex_board, validate_tiles,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt,
    sync::{Arc, OnceLock},
};

#[derive(Debug, Eq, PartialEq)]
pub enum ProfileError {
    InvalidIdentity(String),
    InvalidWordList(String),
    InvalidTiles(String),
    InvalidBoard(String),
    InvalidMatchRules(String),
    UnsupportedLetters { letters: Vec<char> },
    Encoding(String),
}
impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(reason) => write!(f, "Invalid profile identity: {reason}"),
            Self::InvalidWordList(reason) => write!(f, "Invalid word list: {reason}"),
            Self::InvalidTiles(reason) => write!(f, "Invalid tile profile: {reason}"),
            Self::InvalidBoard(reason) => write!(f, "Invalid board profile: {reason}"),
            Self::InvalidMatchRules(reason) => write!(f, "Invalid match settings: {reason}"),
            Self::UnsupportedLetters { letters } => {
                write!(
                    f,
                    "Word list requires letters absent from the tile profile: {letters:?}"
                )
            }
            Self::Encoding(reason) => write!(f, "Could not encode profile identity: {reason}"),
        }
    }
}
impl std::error::Error for ProfileError {}

fn content_identity<T: Serialize>(
    source: &ContentIdentity,
    kind: &str,
    content: &T,
) -> Result<ContentIdentity, ProfileError> {
    source
        .validate()
        .map_err(|error| ProfileError::InvalidIdentity(error.to_string()))?;
    let bytes = serde_json::to_vec(&(kind, source, content))
        .map_err(|error| ProfileError::Encoding(error.to_string()))?;
    Ok(ContentIdentity {
        name: source.name.clone(),
        revision: format!(
            "1+{kind}-sha256:{}",
            hexadecimal(digest::digest(&digest::SHA256, &bytes).as_ref())
        ),
    })
}

/// Prepared once; composing a new match shares the validated dictionary and its alphabet.
pub struct WordListProfile {
    source: ContentIdentity,
    content: OnceLock<(Arc<[u8]>, String)>,
    dictionary: Arc<WordList>,
    alphabet: BTreeSet<char>,
}
impl WordListProfile {
    pub fn new(words: WordList) -> Result<Self, ProfileError> {
        let source = words.identity().clone();
        source
            .validate()
            .map_err(|error| ProfileError::InvalidIdentity(error.to_string()))?;
        let words = fingerprint_word_list(words)
            .map_err(|error| ProfileError::InvalidWordList(error.to_string()))?;
        let identity = content_identity(&source, "word-profile", &words.identity().revision)?;
        let words = words
            .with_revision(identity.revision)
            .map_err(|error| ProfileError::InvalidWordList(error.to_string()))?;
        let alphabet = words.words().flat_map(str::chars).collect();
        Ok(Self {
            dictionary: Arc::new(words),
            source,
            content: OnceLock::new(),
            alphabet,
        })
    }
    pub fn identity(&self) -> &ContentIdentity {
        self.dictionary.identity()
    }
    pub(crate) fn source(&self) -> &ContentIdentity {
        &self.source
    }
    pub(crate) fn content(&self) -> (Arc<[u8]>, String) {
        self.content
            .get_or_init(|| {
                let mut bytes = Vec::new();
                for word in self.dictionary.words() {
                    bytes.extend_from_slice(word.as_bytes());
                    bytes.push(b'\n');
                }
                let hash = hexadecimal(digest::digest(&digest::SHA256, &bytes).as_ref());
                (Arc::from(bytes), hash)
            })
            .clone()
    }
}

pub struct TileProfile {
    source: ContentIdentity,
    identity: ContentIdentity,
    tiles: Vec<TileDefinition>,
    alphabet: BTreeSet<char>,
}
impl TileProfile {
    pub fn new(
        identity: ContentIdentity,
        tiles: Vec<TileDefinition>,
    ) -> Result<Self, ProfileError> {
        validate_tiles(&tiles).map_err(|error| ProfileError::InvalidTiles(error.to_string()))?;
        let source = identity;
        let identity = content_identity(&source, "tile-profile", &tiles)?;
        let alphabet = tiles
            .iter()
            // Every letter of every non-blank face, including multi-letter faces (#116).
            .filter_map(|tile| tile.face.spelled())
            .flat_map(|face| face.chars().collect::<Vec<_>>())
            .collect();
        Ok(Self {
            source,
            identity,
            tiles,
            alphabet,
        })
    }
    pub(crate) fn source(&self) -> &ContentIdentity {
        &self.source
    }
    pub(crate) fn tiles(&self) -> &[TileDefinition] {
        &self.tiles
    }
    pub fn identity(&self) -> &ContentIdentity {
        &self.identity
    }
}

/// Validated immutable board geometry and its scoring policies.
pub struct BoardProfile {
    source: ContentIdentity,
    identity: ContentIdentity,
    board_size: u8,
    premiums: Vec<Premium>,
    shape: BoardShape,
    policy: PremiumPolicy,
    metric: WordLengthMetric,
}
impl BoardProfile {
    pub fn new(
        identity: ContentIdentity,
        board_size: u8,
        premiums: Vec<Premium>,
    ) -> Result<Self, ProfileError> {
        Self::with_geometry(
            identity,
            board_size,
            premiums,
            BoardShape::Square,
            PremiumPolicy::AllWord,
            WordLengthMetric::Tiles,
        )
    }
    pub fn with_geometry(
        identity: ContentIdentity,
        board_size: u8,
        premiums: Vec<Premium>,
        shape: BoardShape,
        policy: PremiumPolicy,
        metric: WordLengthMetric,
    ) -> Result<Self, ProfileError> {
        match shape {
            BoardShape::Square => {
                if policy != PremiumPolicy::AllWord || metric != WordLengthMetric::Tiles {
                    return Err(ProfileError::InvalidBoard(
                        "hex policies require hex geometry".into(),
                    ));
                }
                validate_board(board_size, &premiums)
            }
            BoardShape::Hexagon => validate_hex_board(board_size, &premiums),
        }
        .map_err(|error| ProfileError::InvalidBoard(error.to_string()))?;
        let source = identity;
        // Preserve every established square profile fingerprint exactly.
        let identity = if shape == BoardShape::Square {
            content_identity(&source, "board-profile", &(board_size, &premiums))?
        } else {
            content_identity(
                &source,
                "board-profile",
                &(shape, board_size, &premiums, policy, metric),
            )?
        };
        Ok(Self {
            source,
            identity,
            board_size,
            premiums,
            shape,
            policy,
            metric,
        })
    }
    pub(crate) fn source(&self) -> &ContentIdentity {
        &self.source
    }
    pub(crate) fn size(&self) -> u8 {
        self.board_size
    }
    pub(crate) fn premiums(&self) -> &[Premium] {
        &self.premiums
    }
    pub(crate) fn shape(&self) -> BoardShape {
        self.shape
    }
    pub(crate) fn policy(&self) -> PremiumPolicy {
        self.policy
    }
    pub(crate) fn metric(&self) -> WordLengthMetric {
        self.metric
    }
    pub fn identity(&self) -> &ContentIdentity {
        &self.identity
    }
}

/// Explicit effective values; no new match defaults are introduced by composition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchRules {
    pub rack_size: u8,
    pub bingo_bonus: u16,
    pub exchange_minimum_bag: u16,
    pub scoreless_turn_limit: u16,
    pub minimum_players: u8,
    pub maximum_players: u8,
    /// Host's fixed match option (Decision 1.53); absent means off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dictionary_hints: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSelection {
    pub word_list: ContentIdentity,
    pub tile: ContentIdentity,
    pub board: ContentIdentity,
}

/// Validated engine inputs. No catalog, match allocation or source mutation occurs here.
pub struct ComposedRuleset {
    pub rules: Ruleset,
    pub dictionary: Arc<WordList>,
    pub selection: ProfileSelection,
}
pub fn compose_profiles(
    word_list: &WordListProfile,
    tile: &TileProfile,
    board: &BoardProfile,
    settings: MatchRules,
) -> Result<ComposedRuleset, ProfileError> {
    compose_timed_profiles(word_list, tile, board, settings, None)
}

/// `compose_profiles` with an optional, immutable match time control (#114). The time
/// control is part of the effective ruleset and therefore of its composed identity.
pub fn compose_timed_profiles(
    word_list: &WordListProfile,
    tile: &TileProfile,
    board: &BoardProfile,
    settings: MatchRules,
    time_control: Option<scrabble_game::ruleset::TimeControl>,
) -> Result<ComposedRuleset, ProfileError> {
    let unsupported: Vec<_> = word_list
        .alphabet
        .difference(&tile.alphabet)
        .copied()
        .collect();
    if !unsupported.is_empty() {
        return Err(ProfileError::UnsupportedLetters {
            letters: unsupported,
        });
    }
    let selection = ProfileSelection {
        word_list: word_list.identity().clone(),
        tile: tile.identity.clone(),
        board: board.identity.clone(),
    };
    let mut rules = Ruleset {
        identity: ContentIdentity {
            name: "composed-profiles".into(),
            revision: "1".into(),
        },
        dictionary: word_list.identity().clone(),
        tiles: tile.tiles.clone(),
        board_size: board.board_size,
        premiums: board.premiums.clone(),
        rack_size: settings.rack_size,
        bingo_bonus: settings.bingo_bonus,
        exchange_minimum_bag: settings.exchange_minimum_bag,
        scoreless_turn_limit: settings.scoreless_turn_limit,
        minimum_players: settings.minimum_players,
        maximum_players: settings.maximum_players,
        board_shape: board.shape,
        premium_policy: board.policy,
        main_word_length: board.metric,
        time_control,
        dictionary_hints: settings.dictionary_hints,
    };
    rules
        .validate()
        .map_err(|error| ProfileError::InvalidMatchRules(error.to_string()))?;
    rules.identity = content_identity(&rules.identity, "composition", &(&selection, &rules))?;
    Ok(ComposedRuleset {
        rules,
        dictionary: Arc::clone(&word_list.dictionary),
        selection,
    })
}
