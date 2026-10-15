//! Immutable profile composition; gameplay remains owned by scrabble-game.
use crate::dictionary::{fingerprint_word_list, hexadecimal};
use ring::digest;
use scrabble_game::{
    dictionary::{Dictionary, WordList},
    ruleset::{
        ContentIdentity, Premium, Ruleset, TileDefinition, TileFace, validate_board, validate_tiles,
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
            .filter_map(|tile| match tile.face {
                TileFace::Letter { letter } => Some(letter),
                TileFace::Blank {} => None,
            })
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

/// The existing square-board contract. Hex geometry is a separate #96 capability.
pub struct BoardProfile {
    source: ContentIdentity,
    identity: ContentIdentity,
    board_size: u8,
    premiums: Vec<Premium>,
}
impl BoardProfile {
    pub fn new(
        identity: ContentIdentity,
        board_size: u8,
        premiums: Vec<Premium>,
    ) -> Result<Self, ProfileError> {
        validate_board(board_size, &premiums)
            .map_err(|error| ProfileError::InvalidBoard(error.to_string()))?;
        let source = identity;
        let identity = content_identity(&source, "board-profile", &(board_size, &premiums))?;
        Ok(Self {
            source,
            identity,
            board_size,
            premiums,
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
        board_shape: Default::default(),
        premium_policy: Default::default(),
        main_word_length: Default::default(),
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
