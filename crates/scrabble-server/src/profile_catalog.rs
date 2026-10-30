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
    ruleset::{
        BoardShape, ContentIdentity, Premium, PremiumPolicy, Ruleset, TileDefinition, TileFace,
        WordLengthMetric, english_fixture,
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

/// One prepared word-list file.
const MAX_LIST_BYTES: u64 = 64 * 1024 * 1024;
/// All catalog word lists together (#216: ten operator presets plus the built-in list).
const MAX_CATALOG_WORD_BYTES: usize = 160 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub word_list: ContentIdentity,
    pub tile: ContentIdentity,
    pub board: ContentIdentity,
}
/// A version-3 creation selection. `word_list_access` proves access to a guest word
/// list for a NEW match (Decision 1.48); it is never persisted, so started matches keep
/// their manifest selection unchanged.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestedSelection {
    pub word_list: ContentIdentity,
    pub tile: ContentIdentity,
    pub board: ContentIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_list_access: Option<crate::word_list_library::WordListAccess>,
}
impl RequestedSelection {
    pub fn selection(&self) -> Selection {
        Selection {
            word_list: self.word_list.clone(),
            tile: self.tile.clone(),
            board: self.board.clone(),
        }
    }
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
            dictionary_hints: rules.dictionary_hints,
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
    #[serde(default)]
    board_shape: BoardShape,
    #[serde(default)]
    premium_policy: PremiumPolicy,
    #[serde(default)]
    main_word_length: WordLengthMetric,
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
    /// Starts from the built-in profiles of the deployment dictionary and ruleset, so an
    /// operator adds presets (#153) without restating the built-in boards and tiles.
    #[serde(default)]
    include_builtin: bool,
    /// Label of the built-in deployment word list; only with `includeBuiltin`.
    #[serde(default)]
    builtin_word_list_label: Option<String>,
    #[serde(default)]
    word_lists: Vec<WordSource>,
    #[serde(default)]
    tile_profiles: Vec<TileSource>,
    #[serde(default)]
    board_profiles: Vec<BoardSource>,
    /// Required without `includeBuiltin`; otherwise defaults to the built-in selection.
    #[serde(default)]
    default_selection: Option<DefaultNames>,
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

type Profiles = (
    Vec<Named<WordListProfile>>,
    Vec<Named<TileProfile>>,
    Vec<Named<BoardProfile>>,
    DefaultNames,
);
/// The built-in profiles of the deployment dictionary and ruleset.
fn builtin(legacy: &Ruleset, dictionary: &WordList) -> ErrorResult<Profiles> {
    let words = WordListProfile::new(dictionary.clone())?;
    let tiles = TileProfile::new(legacy.identity.clone(), legacy.tiles.clone())?;
    // Offer centered, smaller square boards without inventing a second rules engine.
    // Keep the original profile as the untouched default for v2 creation/recovery.
    let mut boards = Vec::new();
    for size in (5..legacy.board_size).step_by(2) {
        let name = format!("builtin-square-{size}");
        if name == legacy.identity.name {
            continue;
        }
        let offset = usize::from((legacy.board_size - size) / 2);
        let original_size = usize::from(legacy.board_size);
        let side = usize::from(size);
        let premiums = (0..side)
            .flat_map(|row| {
                (0..side).map(move |column| {
                    legacy.premiums[(row + offset) * original_size + column + offset]
                })
            })
            .collect();
        boards.push(Named {
            label: format!("Square {size} × {size}"),
            profile: BoardProfile::new(
                ContentIdentity {
                    name,
                    revision: "1".into(),
                },
                size,
                premiums,
            )?,
        });
    }
    boards.push(Named {
        label: "Default board".into(),
        profile: BoardProfile::new(
            legacy.identity.clone(),
            legacy.board_size,
            legacy.premiums.clone(),
        )?,
    });
    for (name, label, policy) in [
        (
            "builtin-hex-all-word",
            "Hex all-word",
            PremiumPolicy::AllWord,
        ),
        (
            "builtin-hex-main-word",
            "Hex main-word-only",
            PremiumPolicy::MainWordOnly,
        ),
    ] {
        for (suffix, metric_label, metric) in [
            ("", "tiles", WordLengthMetric::Tiles),
            ("-letters", "letters", WordLengthMetric::Letters),
        ] {
            let mut premiums = vec![Premium::Normal; 37];
            // Row-major on-board cells of the radius-three board.
            premiums[18] = Premium::DoubleWord;
            // Interior (3, 2) remains available for a later crossing-word premium.
            premiums[17] = Premium::DoubleWord;
            boards.push(Named {
                label: format!("{label} ({metric_label})"),
                profile: BoardProfile::with_geometry(
                    ContentIdentity {
                        name: format!("{name}{suffix}"),
                        revision: "1".into(),
                    },
                    7,
                    premiums,
                    BoardShape::Hexagon,
                    policy,
                    metric,
                )?,
            });
        }
    }
    let names = DefaultNames {
        word_list: words.identity().name.clone(),
        tile: tiles.identity().name.clone(),
        board: legacy.identity.name.clone(),
    };
    let mut tile_profiles = vec![Named {
        label: "Default tiles".into(),
        profile: tiles,
    }];
    // This preset extends the known English distribution, never an operator's
    // custom alphabet. Its immutable identity covers all counts and values.
    let multiletter_name = "builtin-english-ch-sch";
    if legacy.tiles == english_fixture().tiles && legacy.identity.name != multiletter_name {
        let mut tiles = legacy.tiles.clone();
        // Rare combined faces retain the summed value of their ordinary letters.
        for (letters, count, value) in [("CH", 2, 7), ("SCH", 1, 8)] {
            tiles.push(TileDefinition {
                face: TileFace::Letters {
                    letters: letters.into(),
                },
                count,
                value,
            });
        }
        tile_profiles.push(Named {
            label: "English + CH / SCH".into(),
            profile: TileProfile::new(
                ContentIdentity {
                    name: multiletter_name.into(),
                    revision: "1".into(),
                },
                tiles,
            )?,
        });
    }
    Ok((
        vec![Named {
            label: dictionary.identity().name.clone(),
            profile: words,
        }],
        tile_profiles,
        boards,
        names,
    ))
}
fn from_file(path: &Path, legacy: &Ruleset, dictionary: &WordList) -> ErrorResult<Profiles> {
    let raw: SourceCatalog = serde_json::from_slice(&read_bounded(path, 1024 * 1024)?)?;
    if raw.version != 1 {
        return Err("unsupported catalog version".into());
    }
    let (mut words, mut tiles, mut boards, builtin_names) = if raw.include_builtin {
        let (mut words, tiles, boards, names) = builtin(legacy, dictionary)?;
        if let (Some(label), Some(row)) = (raw.builtin_word_list_label, words.first_mut()) {
            row.label = label;
        }
        (words, tiles, boards, Some(names))
    } else {
        if raw.builtin_word_list_label.is_some() {
            return Err("builtinWordListLabel requires includeBuiltin".into());
        }
        (Vec::new(), Vec::new(), Vec::new(), None)
    };
    let directory = path.parent().unwrap_or(Path::new("."));
    let mut total = words
        .iter()
        .map(|row| row.profile.content().0.len())
        .sum::<usize>();
    for row in raw.word_lists {
        let bytes = read_bounded(&directory.join(row.file), MAX_LIST_BYTES)?;
        total = total
            .checked_add(bytes.len())
            .ok_or("catalog words exceed supported size")?;
        if total > MAX_CATALOG_WORD_BYTES {
            return Err("catalog words exceed 160 MiB total".into());
        }
        let source = WordList::from_text(row.identity, std::str::from_utf8(&bytes)?)?;
        words.push(Named {
            label: row.label,
            profile: WordListProfile::new(source)?,
        });
    }
    for r in raw.tile_profiles {
        tiles.push(Named {
            label: r.label,
            profile: TileProfile::new(r.identity, r.tiles)?,
        });
    }
    for r in raw.board_profiles {
        boards.push(Named {
            label: r.label,
            profile: BoardProfile::with_geometry(
                r.identity,
                r.board_size,
                r.premiums,
                r.board_shape,
                r.premium_policy,
                r.main_word_length,
            )?,
        });
    }
    validate_rows(words.iter().map(|r| (r.label.as_str(), r.profile.source())))?;
    validate_rows(tiles.iter().map(|r| (r.label.as_str(), r.profile.source())))?;
    validate_rows(
        boards
            .iter()
            .map(|r| (r.label.as_str(), r.profile.source())),
    )?;
    let names = raw
        .default_selection
        .or(builtin_names)
        .ok_or("catalog requires a defaultSelection")?;
    Ok((words, tiles, boards, names))
}

pub(crate) struct ProfileCatalog {
    words: Vec<Named<WordListProfile>>,
    tiles: Vec<Named<TileProfile>>,
    boards: Vec<Named<BoardProfile>>,
    public: serde_json::Value,
    legacy_public: Option<serde_json::Value>,
}
impl ProfileCatalog {
    pub fn load(path: Option<&Path>, legacy: &Ruleset, dictionary: &WordList) -> ErrorResult<Self> {
        let (words, tiles, boards, names) = match path {
            Some(path) => from_file(path, legacy, dictionary)?,
            None => builtin(legacy, dictionary)?,
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
            legacy_public: None,
        };
        let prepared = catalog
            .resolve(&selection, MatchRules::from(legacy))
            .map_err(|_| "default profile combination is invalid")?;
        let legacy_compatible = |composed: &ComposedRuleset| {
            composed.rules.tiles == legacy.tiles
                && composed.rules.board_shape == BoardShape::Square
                && composed.rules.board_size == legacy.board_size
                && composed.rules.premiums == legacy.premiums
                && composed.dictionary.words().eq(dictionary.words())
        };
        let square_selection = catalog
            .boards
            .iter()
            .find(|row| {
                row.profile.shape() == BoardShape::Square
                    && row.profile.identity() == &selection.board
            })
            .or_else(|| {
                catalog
                    .boards
                    .iter()
                    .find(|row| row.profile.shape() == BoardShape::Square)
            })
            .map(|row| Selection {
                board: row.profile.identity().clone(),
                ..selection.clone()
            });
        catalog.public = serde_json::json!({"version":1,
            "wordLists":catalog.words.iter().map(|r|serde_json::json!({"identity":r.profile.identity(),"label":r.label})).collect::<Vec<_>>(),
            "tileProfiles":catalog.tiles.iter().map(|r|serde_json::json!({"identity":r.profile.identity(),"label":r.label,"tiles":r.profile.tiles()})).collect::<Vec<_>>(),
            "boardProfiles":catalog.boards.iter().map(|r| {
                let mut row = serde_json::json!({"identity":r.profile.identity(),"label":r.label,"boardSize":r.profile.size(),"premiums":r.profile.premiums()});
                if r.profile.shape() == BoardShape::Hexagon {
                    row["boardShape"] = serde_json::json!(r.profile.shape());
                    row["premiumPolicy"] = serde_json::json!(r.profile.policy());
                    row["mainWordLength"] = serde_json::json!(r.profile.metric());
                }
                row
            }).collect::<Vec<_>>(),
            "defaults":{"selection":selection,"rules":MatchRules::from(legacy),"legacyCompatible":legacy_compatible(&prepared.composed)}});
        if let Some(square_selection) = square_selection {
            let square = catalog
                .resolve(&square_selection, MatchRules::from(legacy))
                .map_err(|_| "legacy square profile combination is invalid")?;
            let mut projection = catalog.public.clone();
            projection["boardProfiles"]
                .as_array_mut()
                .ok_or("invalid public board profiles")?
                .retain(|row| row.get("boardShape").is_none());
            projection["defaults"] = serde_json::json!({
                "selection": square_selection,
                "rules": MatchRules::from(legacy),
                "legacyCompatible": legacy_compatible(&square.composed),
            });
            catalog.legacy_public = Some(projection);
        }
        if serde_json::to_vec(&catalog.public)?.len() > 128 * 1024 {
            return Err("public profile catalog exceeds 128 KiB".into());
        }
        Ok(catalog)
    }
    pub fn public(&self) -> Option<serde_json::Value> {
        self.legacy_public.clone()
    }
    pub fn public_hex(&self) -> serde_json::Value {
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
        // Keep the established error order: word list, tiles, then board.
        let words = self
            .word_list(&selection.word_list)
            .ok_or(SelectionError::Unknown("wordList"))?;
        let tiles = self
            .tile(&selection.tile)
            .ok_or(SelectionError::Unknown("tile"))?;
        let board = self
            .board(&selection.board)
            .ok_or(SelectionError::Unknown("board"))?;
        self.compose(selection, words, tiles, board, rules, time_control)
    }
    pub fn word_list(&self, identity: &ContentIdentity) -> Option<&WordListProfile> {
        self.words
            .iter()
            .find(|r| r.profile.identity() == identity)
            .map(|r| &r.profile)
    }
    pub fn tile(&self, identity: &ContentIdentity) -> Option<&TileProfile> {
        self.tiles
            .iter()
            .map(|r| &r.profile)
            .find(|profile| profile.identity() == identity)
    }
    pub fn board(&self, identity: &ContentIdentity) -> Option<&BoardProfile> {
        self.boards
            .iter()
            .find(|r| r.profile.identity() == identity)
            .map(|r| &r.profile)
    }
    /// Composes an already validated word list, tile profile and board, each from this
    /// catalog or a guest library (#97, #100, #101), under the single compatibility check.
    pub fn compose(
        &self,
        selection: &Selection,
        words: &WordListProfile,
        tiles: &TileProfile,
        board: &BoardProfile,
        rules: MatchRules,
        time_control: Option<scrabble_game::ruleset::TimeControl>,
    ) -> Result<PreparedConfiguration, SelectionError> {
        if words.identity() != &selection.word_list {
            return Err(SelectionError::Unknown("wordList"));
        }
        if tiles.identity() != &selection.tile {
            return Err(SelectionError::Unknown("tile"));
        }
        if board.identity() != &selection.board {
            return Err(SelectionError::Unknown("board"));
        }
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

#[cfg(test)]
mod board_size_tests {
    use super::*;
    use scrabble_game::ruleset::english_fixture;

    #[test]
    fn builtin_resized_boards_are_authoritative_and_preserve_the_legacy_default() {
        let original = english_fixture();
        let dictionary = WordList::from_text(original.dictionary.clone(), "CAT\nAT\n").unwrap();
        let catalog = ProfileCatalog::load(None, &original, &dictionary).unwrap();
        let public = catalog.public().unwrap();
        assert_eq!(public["defaults"]["legacyCompatible"], true);
        let boards = public["boardProfiles"].as_array().unwrap();
        let sizes: Vec<u64> = boards
            .iter()
            .map(|row| row["boardSize"].as_u64().unwrap())
            .collect();
        assert_eq!(sizes, [5, 7, 9, 11, 13, 15]);

        let defaults: Selection =
            serde_json::from_value(public["defaults"]["selection"].clone()).unwrap();
        for profile in boards {
            let size = profile["boardSize"].as_u64().unwrap() as usize;
            let mut selection = defaults.clone();
            selection.board = serde_json::from_value(profile["identity"].clone()).unwrap();
            let prepared = catalog
                .resolve(&selection, MatchRules::from(&original))
                .unwrap();
            let rules = prepared.composed.rules;
            assert_eq!(rules.board_size as usize, size);
            let offset = (original.board_size as usize - size) / 2;
            for row in 0..size {
                for column in 0..size {
                    assert_eq!(
                        rules.premiums[row * size + column],
                        original.premiums
                            [(row + offset) * original.board_size as usize + column + offset]
                    );
                }
            }
        }
    }
    /// Retrospective #96 sample acceptance: policy choices must have a usable
    /// crossing-word premium after the mandatory centre-covering opening.
    /// Existing native tests prove the scoring rules; this checks REAL fallback
    /// catalog profiles rather than acceptance-only authored board layouts.
    #[test]
    fn builtin_hex_samples_keep_classic_defaults_and_have_reusable_policy_demonstrations() {
        let original = english_fixture();
        let dictionary = WordList::from_text(original.dictionary.clone(), "CAT\nAT\n").unwrap();
        let catalog = ProfileCatalog::load(None, &original, &dictionary).unwrap();
        let legacy = catalog.public().unwrap();
        let extended = catalog.public_hex();
        assert_eq!(extended["defaults"], legacy["defaults"]);
        assert_eq!(legacy["defaults"]["legacyCompatible"], true);
        let defaults: Selection =
            serde_json::from_value(legacy["defaults"]["selection"].clone()).unwrap();
        let classic = catalog
            .resolve(&defaults, MatchRules::from(&original))
            .unwrap()
            .composed
            .rules;
        assert_eq!(classic.board_shape, BoardShape::Square);
        assert_eq!(classic.board_size, original.board_size);
        assert_eq!(classic.premiums, original.premiums);

        let hex: Vec<_> = extended["boardProfiles"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["boardShape"] == "hexagon")
            .collect();
        assert_eq!(hex.len(), 4);
        let mut selected = Vec::new();
        for profile in hex {
            let mut selection = defaults.clone();
            selection.board = serde_json::from_value(profile["identity"].clone()).unwrap();
            let rules = catalog
                .resolve(&selection, MatchRules::from(&original))
                .unwrap()
                .composed
                .rules;
            assert_eq!(rules.board_shape, BoardShape::Hexagon);
            selected.push(rules);
        }
        for policy in [PremiumPolicy::AllWord, PremiumPolicy::MainWordOnly] {
            for metric in [WordLengthMetric::Tiles, WordLengthMetric::Letters] {
                assert_eq!(
                    selected
                        .iter()
                        .filter(|rules| rules.premium_policy == policy
                            && rules.main_word_length == metric)
                        .count(),
                    1
                );
            }
        }
        for rules in &selected {
            assert_eq!(rules.board_size, selected[0].board_size);
            assert_eq!(rules.premiums, selected[0].premiums);
            let center = rules.board_size / 2;
            assert_eq!(rules.premium_at(center, center), Some(Premium::DoubleWord));
            let usable = (1..rules.board_size-1).any(|row| (1..rules.board_size-1).any(|column| {
                (row, column) != (center, center)
                    && matches!(rules.premium_at(row, column), Some(Premium::DoubleWord | Premium::TripleWord))
                    // A complete interior neighbourhood supports genuine
                    // intersecting words along all three native Hex axes.
                    && [(0_i16,1_i16),(0,-1),(1,0),(-1,0),(1,-1),(-1,1)].iter().all(|&(dr,dc)|
                        rules.contains((i16::from(row)+dr) as u8, (i16::from(column)+dc) as u8))
            }));
            assert!(
                usable,
                "{} has only opening-consumed word premiums; its policy cannot change a later crossing-word score",
                rules.identity.name
            );
        }
    }
}
