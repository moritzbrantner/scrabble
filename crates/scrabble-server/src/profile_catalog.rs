//! Operator-owned source catalog. Public metadata never includes word contents or source paths.
use crate::{
    ErrorResult,
    profiles::{
        BoardProfile, ComposedRuleset, MatchRules, ProfileError, ProfileSelection, TileProfile,
        WordListProfile,
    },
};
use scrabble_game::{
    dictionary::WordList,
    ruleset::{ContentIdentity, Premium, Ruleset, TileDefinition},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub word_list: ContentIdentity,
    pub tile: ContentIdentity,
    pub board: ContentIdentity,
}
impl From<&ProfileSelection> for Selection {
    fn from(value: &ProfileSelection) -> Self {
        Self {
            word_list: value.word_list.clone(),
            tile: value.tile.clone(),
            board: value.board.clone(),
        }
    }
}
impl From<&Ruleset> for MatchRules {
    fn from(rules: &Ruleset) -> Self {
        Self {
            rack_size: rules.rack_size,
            bingo_bonus: rules.bingo_bonus,
            exchange_minimum_bag: rules.exchange_minimum_bag,
            scoreless_turn_limit: rules.scoreless_turn_limit,
            minimum_players: rules.minimum_players,
            maximum_players: rules.maximum_players,
        }
    }
}
#[derive(Debug)]
pub enum SelectionError {
    Unknown(&'static str),
    Incompatible(Vec<char>),
    InvalidRules,
}

pub(crate) struct PreparedConfiguration {
    pub composed: ComposedRuleset,
    pub sources: ProfileSelection,
    pub word_content: std::sync::Arc<[u8]>,
    pub word_hash: String,
}
struct Named<T> {
    label: String,
    profile: T,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WordSource {
    identity: ContentIdentity,
    label: String,
    file: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TileSource {
    identity: ContentIdentity,
    label: String,
    tiles: Vec<TileDefinition>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BoardSource {
    identity: ContentIdentity,
    label: String,
    board_size: u8,
    premiums: Vec<Premium>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DefaultNames {
    word_list: String,
    tile: String,
    board: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceCatalog {
    version: u8,
    word_lists: Vec<WordSource>,
    tile_profiles: Vec<TileSource>,
    board_profiles: Vec<BoardSource>,
    default_selection: DefaultNames,
}

pub(crate) fn read_bounded(path: &Path, limit: u64) -> ErrorResult<Vec<u8>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("profile input must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("profile input exceeds its supported size".into());
    }
    Ok(bytes)
}
fn validate_rows<'a>(
    rows: impl Iterator<Item = (&'a str, &'a ContentIdentity)>,
) -> ErrorResult<()> {
    let mut names = BTreeSet::new();
    let mut count = 0;
    for (label, identity) in rows {
        if label.is_empty()
            || label.len() > 512
            || label.chars().any(char::is_control)
            || !names.insert(&identity.name)
        {
            return Err("profile labels must be bounded text and source names unique".into());
        }
        count += 1;
    }
    if count == 0 || count > 32 {
        return Err("catalog requires one to 32 profiles of each kind".into());
    }
    Ok(())
}

pub(crate) struct ProfileCatalog {
    words: Vec<Named<WordListProfile>>,
    tiles: Vec<Named<TileProfile>>,
    boards: Vec<Named<BoardProfile>>,
    public: serde_json::Value,
}
impl ProfileCatalog {
    pub fn load(path: Option<&Path>, legacy: &Ruleset, dictionary: &WordList) -> ErrorResult<Self> {
        let (words, tiles, boards, names) = if let Some(path) = path {
            let raw: SourceCatalog = serde_json::from_slice(&read_bounded(path, 1024 * 1024)?)?;
            if raw.version != 1 {
                return Err("unsupported catalog version".into());
            }
            validate_rows(
                raw.word_lists
                    .iter()
                    .map(|r| (r.label.as_str(), &r.identity)),
            )?;
            validate_rows(
                raw.tile_profiles
                    .iter()
                    .map(|r| (r.label.as_str(), &r.identity)),
            )?;
            validate_rows(
                raw.board_profiles
                    .iter()
                    .map(|r| (r.label.as_str(), &r.identity)),
            )?;
            let directory = path.parent().unwrap_or(Path::new("."));
            let mut words = Vec::new();
            let mut total = 0_usize;
            for row in raw.word_lists {
                let bytes = read_bounded(&directory.join(row.file), 64 * 1024 * 1024)?;
                total = total
                    .checked_add(bytes.len())
                    .ok_or("catalog words exceed supported size")?;
                if total > 64 * 1024 * 1024 {
                    return Err("catalog words exceed 64 MiB total".into());
                }
                let source = WordList::from_text(row.identity, std::str::from_utf8(&bytes)?)?;
                words.push(Named {
                    label: row.label,
                    profile: WordListProfile::new(source)?,
                });
            }
            let tiles = raw
                .tile_profiles
                .into_iter()
                .map(|r| {
                    Ok(Named {
                        label: r.label,
                        profile: TileProfile::new(r.identity, r.tiles)?,
                    })
                })
                .collect::<ErrorResult<Vec<_>>>()?;
            let boards = raw
                .board_profiles
                .into_iter()
                .map(|r| {
                    Ok(Named {
                        label: r.label,
                        profile: BoardProfile::new(r.identity, r.board_size, r.premiums)?,
                    })
                })
                .collect::<ErrorResult<Vec<_>>>()?;
            (words, tiles, boards, raw.default_selection)
        } else {
            let words = WordListProfile::new(dictionary.clone())?;
            let tiles = TileProfile::new(legacy.identity.clone(), legacy.tiles.clone())?;
            let boards = BoardProfile::new(
                legacy.identity.clone(),
                legacy.board_size,
                legacy.premiums.clone(),
            )?;
            let names = DefaultNames {
                word_list: words.identity().name.clone(),
                tile: tiles.identity().name.clone(),
                board: boards.identity().name.clone(),
            };
            (
                vec![Named {
                    label: dictionary.identity().name.clone(),
                    profile: words,
                }],
                vec![Named {
                    label: "Default tiles".into(),
                    profile: tiles,
                }],
                vec![Named {
                    label: "Default board".into(),
                    profile: boards,
                }],
                names,
            )
        };
        let selection = Selection {
            word_list: words
                .iter()
                .find(|r| r.profile.identity().name == names.word_list)
                .ok_or("unknown default word list")?
                .profile
                .identity()
                .clone(),
            tile: tiles
                .iter()
                .find(|r| r.profile.identity().name == names.tile)
                .ok_or("unknown default tiles")?
                .profile
                .identity()
                .clone(),
            board: boards
                .iter()
                .find(|r| r.profile.identity().name == names.board)
                .ok_or("unknown default board")?
                .profile
                .identity()
                .clone(),
        };
        let mut catalog = Self {
            words,
            tiles,
            boards,
            public: serde_json::Value::Null,
        };
        let prepared = catalog
            .resolve(&selection, MatchRules::from(legacy))
            .map_err(|_| "default profile combination is invalid")?;
        let composed = &prepared.composed;
        let legacy_compatible = composed.rules.tiles == legacy.tiles
            && composed.rules.board_size == legacy.board_size
            && composed.rules.premiums == legacy.premiums
            && composed.dictionary.words().eq(dictionary.words());
        catalog.public = serde_json::json!({"version":1,
            "wordLists":catalog.words.iter().map(|r|serde_json::json!({"identity":r.profile.identity(),"label":r.label})).collect::<Vec<_>>(),
            "tileProfiles":catalog.tiles.iter().map(|r|serde_json::json!({"identity":r.profile.identity(),"label":r.label,"tiles":r.profile.tiles()})).collect::<Vec<_>>(),
            "boardProfiles":catalog.boards.iter().map(|r|serde_json::json!({"identity":r.profile.identity(),"label":r.label,"boardSize":r.profile.size(),"premiums":r.profile.premiums()})).collect::<Vec<_>>(),
            "defaults":{"selection":selection,"rules":MatchRules::from(legacy),"legacyCompatible":legacy_compatible}});
        if serde_json::to_vec(&catalog.public)?.len() > 128 * 1024 {
            return Err("public profile catalog exceeds 128 KiB".into());
        }
        Ok(catalog)
    }
    pub fn public(&self) -> serde_json::Value {
        self.public.clone()
    }
    pub fn resolve(
        &self,
        selection: &Selection,
        rules: MatchRules,
    ) -> Result<PreparedConfiguration, SelectionError> {
        self.resolve_timed(selection, rules, None)
    }
    pub fn resolve_timed(
        &self,
        selection: &Selection,
        rules: MatchRules,
        time_control: Option<scrabble_game::ruleset::TimeControl>,
    ) -> Result<PreparedConfiguration, SelectionError> {
        let words = &self
            .words
            .iter()
            .find(|r| r.profile.identity() == &selection.word_list)
            .ok_or(SelectionError::Unknown("wordList"))?
            .profile;
        let tiles = &self
            .tiles
            .iter()
            .find(|r| r.profile.identity() == &selection.tile)
            .ok_or(SelectionError::Unknown("tile"))?
            .profile;
        let board = &self
            .boards
            .iter()
            .find(|r| r.profile.identity() == &selection.board)
            .ok_or(SelectionError::Unknown("board"))?
            .profile;
        let composed =
            crate::profiles::compose_timed_profiles(words, tiles, board, rules, time_control)
                .map_err(|error| match error {
                    ProfileError::UnsupportedLetters { letters } => {
                        SelectionError::Incompatible(letters)
                    }
                    _ => SelectionError::InvalidRules,
                })?;
        let (word_content, word_hash) = words.content();
        Ok(PreparedConfiguration {
            composed,
            word_content,
            word_hash,
            sources: ProfileSelection {
                word_list: words.source().clone(),
                tile: tiles.source().clone(),
                board: board.source().clone(),
            },
        })
    }
}
use scrabble_game::dictionary::Dictionary;
