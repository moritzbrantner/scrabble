//! Versioned data consumed by the deterministic engine, with no browser or network dependency.
use crate::identity::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentIdentity {
    pub name: String,
    pub revision: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TileFace {
    Letter { letter: char },
    Blank {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TileDefinition {
    pub face: TileFace,
    pub count: u16,
    pub value: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Premium {
    Normal,
    DoubleLetter,
    TripleLetter,
    DoubleWord,
    TripleWord,
}

/// Board geometry. A hexagon of radius R uses axial cells on a `(2R+1)²` grid.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardShape {
    #[default]
    Square,
    Hexagon,
}
/// How newly covered premiums apply when a move forms several words (Decision 1.13).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PremiumPolicy {
    #[default]
    AllWord,
    MainWordOnly,
}
/// Length used to pick the single-tile main word (Decision 1.23).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordLengthMetric {
    #[default]
    Tiles,
    Letters,
}
/// Upper bound for any configured clock duration (24 hours); a safety limit, not a default.
pub const MAX_CLOCK_MS: u64 = 86_400_000;

/// Decision 3.5: a carried per-player budget or a fresh per-turn countdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClockModel {
    Cumulative {
        initial_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        increment_ms: Option<u64>,
    },
    PerTurn {
        allowance_ms: u64,
    },
}
/// Decision 3.4: what happens when the active clock expires.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpiryConsequence {
    Pass,
    Forfeit,
}
/// Optional, immutable match time control (Decision 3.1); absent means untimed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeControl {
    pub clock: ClockModel,
    pub on_expiry: ExpiryConsequence,
}
impl TimeControl {
    pub fn validate(&self) -> Result<(), ContractError> {
        let valid = |ms: u64| (1..=MAX_CLOCK_MS).contains(&ms);
        let ok = match self.clock {
            ClockModel::Cumulative {
                initial_ms,
                increment_ms,
            } => {
                if self.on_expiry == ExpiryConsequence::Pass && increment_ms.is_none() {
                    return Err(ContractError(
                        "cumulative clocks with automatic pass need a positive turn increment"
                            .into(),
                    ));
                }
                valid(initial_ms) && increment_ms.is_none_or(valid)
            }
            ClockModel::PerTurn { allowance_ms } => valid(allowance_ms),
        };
        if !ok {
            return Err(ContractError("clock durations are out of range".into()));
        }
        Ok(())
    }
    /// Balance each player holds when the match starts.
    pub const fn starting_ms(&self) -> u64 {
        match self.clock {
            ClockModel::Cumulative { initial_ms, .. } => initial_ms,
            ClockModel::PerTurn { allowance_ms } => allowance_ms,
        }
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ruleset {
    pub identity: ContentIdentity,
    pub dictionary: ContentIdentity,
    pub tiles: Vec<TileDefinition>,
    pub board_size: u8,
    /// Row-major premiums; the center is the required first-move square.
    pub premiums: Vec<Premium>,
    pub rack_size: u8,
    pub bingo_bonus: u16,
    pub exchange_minimum_bag: u16,
    pub scoreless_turn_limit: u16,
    pub minimum_players: u8,
    pub maximum_players: u8,
    #[serde(default, skip_serializing_if = "is_default")]
    pub board_shape: BoardShape,
    #[serde(default, skip_serializing_if = "is_default")]
    pub premium_policy: PremiumPolicy,
    #[serde(default, skip_serializing_if = "is_default")]
    pub main_word_length: WordLengthMetric,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_control: Option<TimeControl>,
}
impl ContentIdentity {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.name.is_empty()
            || self.name.len() > 64
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            || self.revision.is_empty()
            || self.revision.len() > 192
            || !self
                .revision
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+:".contains(&byte))
        {
            return Err(ContractError(
                "content identity requires bounded identifiers and revisions".into(),
            ));
        }
        Ok(())
    }
}

/// Square-board invariants shared by source profiles and effective match configurations.
pub fn validate_board(board_size: u8, premiums: &[Premium]) -> Result<(), ContractError> {
    if board_size == 0
        || board_size > 15
        || board_size.is_multiple_of(2)
        || premiums.len() != usize::from(board_size).pow(2)
    {
        return Err(ContractError(
            "invalid ruleset dimensions or turn limits".into(),
        ));
    }
    Ok(())
}

/// Number of on-board cells of a hexagon with the given storage-grid side.
pub const fn hex_cell_count(board_size: u8) -> usize {
    let radius = (board_size / 2) as usize;
    3 * radius * radius + 3 * radius + 1
}

/// Whether `(row, column)` lies on a hexagonal board of the given storage-grid side.
pub const fn hex_contains(board_size: u8, row: u8, column: u8) -> bool {
    let radius = board_size / 2;
    let sum = row as u16 + column as u16;
    row < board_size && column < board_size && sum >= radius as u16 && sum <= 3 * radius as u16
}

/// Hexagonal-board invariants: odd storage side `2R+1`, `1 <= R <= 7`, one premium per cell.
pub fn validate_hex_board(board_size: u8, premiums: &[Premium]) -> Result<(), ContractError> {
    if !(3..=15).contains(&board_size)
        || board_size.is_multiple_of(2)
        || premiums.len() != hex_cell_count(board_size)
    {
        return Err(ContractError("invalid hexagonal board geometry".into()));
    }
    Ok(())
}

/// Tile-source invariants, independent of the selected rack size and player count.
pub fn validate_tiles(tiles: &[TileDefinition]) -> Result<u32, ContractError> {
    if tiles.len() > 64 {
        return Err(ContractError(
            "tile alphabet exceeds supported bound".into(),
        ));
    }
    let mut letters = BTreeSet::new();
    let mut blank = false;
    let mut total = 0_u32;
    for tile in tiles {
        if tile.count == 0 {
            return Err(ContractError("empty tile definition".into()));
        }
        total += u32::from(tile.count);
        match tile.face {
            TileFace::Letter { letter } => {
                if !letter.is_alphabetic() || !letters.insert(letter) {
                    return Err(ContractError("invalid or duplicate alphabet letter".into()));
                }
            }
            TileFace::Blank {} => {
                if blank || tile.value != 0 {
                    return Err(ContractError("invalid blank definition".into()));
                }
                blank = true;
            }
        }
    }
    if letters.is_empty() || total > 200 {
        return Err(ContractError("invalid tile distribution".into()));
    }
    Ok(total)
}

impl Ruleset {
    /// Whether a storage-grid cell is part of this board.
    pub fn contains(&self, row: u8, column: u8) -> bool {
        match self.board_shape {
            BoardShape::Square => row < self.board_size && column < self.board_size,
            BoardShape::Hexagon => hex_contains(self.board_size, row, column),
        }
    }
    /// Premium of an on-board cell (row-major over on-board cells).
    pub fn premium_at(&self, row: u8, column: u8) -> Option<Premium> {
        if !self.contains(row, column) {
            return None;
        }
        let index = match self.board_shape {
            BoardShape::Square => {
                usize::from(row) * usize::from(self.board_size) + usize::from(column)
            }
            BoardShape::Hexagon => {
                let before: usize = (0..row)
                    .map(|r| {
                        (0..self.board_size)
                            .filter(|&c| self.contains(r, c))
                            .count()
                    })
                    .sum();
                before + (0..column).filter(|&c| self.contains(row, c)).count()
            }
        };
        self.premiums.get(index).copied()
    }
    pub fn validate(&self) -> Result<(), ContractError> {
        self.identity.validate()?;
        self.dictionary.validate()?;
        match self.board_shape {
            BoardShape::Square => {
                validate_board(self.board_size, &self.premiums)?;
                if self.premium_policy != PremiumPolicy::default()
                    || self.main_word_length != WordLengthMetric::default()
                {
                    return Err(ContractError(
                        "premium policy and main-word length apply only to hexagonal boards".into(),
                    ));
                }
            }
            BoardShape::Hexagon => validate_hex_board(self.board_size, &self.premiums)?,
        }
        if let Some(control) = &self.time_control {
            control.validate()?;
        }
        if self.rack_size == 0
            || self.rack_size > 15
            || self.scoreless_turn_limit == 0
            || self.minimum_players < 2
            || self.maximum_players < self.minimum_players
            || self.maximum_players > 4
        {
            return Err(ContractError(
                "invalid ruleset dimensions or turn limits".into(),
            ));
        }
        let total = validate_tiles(&self.tiles)?;
        if total < u32::from(self.maximum_players) * u32::from(self.rack_size) {
            return Err(ContractError("invalid tile distribution".into()));
        }
        Ok(())
    }
}

/// English classic tile/scoring configuration with a deliberately tiny, authored test dictionary.
/// Not a tournament dictionary or a licensed commercial word list.
pub fn english_fixture() -> Ruleset {
    let counts = [
        9, 2, 2, 4, 12, 2, 3, 2, 9, 1, 1, 4, 2, 6, 8, 2, 1, 6, 4, 6, 4, 2, 2, 1, 2, 1,
    ];
    let values = [
        1, 3, 3, 2, 1, 4, 2, 4, 1, 8, 5, 1, 3, 1, 1, 3, 10, 1, 1, 1, 1, 4, 4, 8, 4, 10,
    ];
    let mut tiles: Vec<_> = ('A'..='Z')
        .zip(counts)
        .zip(values)
        .map(|((letter, count), value)| TileDefinition {
            face: TileFace::Letter { letter },
            count,
            value,
        })
        .collect();
    tiles.push(TileDefinition {
        face: TileFace::Blank {},
        count: 2,
        value: 0,
    });
    let mut premiums = vec![Premium::Normal; 225];
    for (premium, squares) in [
        (
            Premium::TripleWord,
            vec![
                (0, 0),
                (0, 7),
                (0, 14),
                (7, 0),
                (7, 14),
                (14, 0),
                (14, 7),
                (14, 14),
            ],
        ),
        (
            Premium::DoubleWord,
            vec![
                (1, 1),
                (2, 2),
                (3, 3),
                (4, 4),
                (7, 7),
                (10, 10),
                (11, 11),
                (12, 12),
                (13, 13),
                (1, 13),
                (2, 12),
                (3, 11),
                (4, 10),
                (10, 4),
                (11, 3),
                (12, 2),
                (13, 1),
            ],
        ),
        (
            Premium::TripleLetter,
            vec![
                (1, 5),
                (1, 9),
                (5, 1),
                (5, 5),
                (5, 9),
                (5, 13),
                (9, 1),
                (9, 5),
                (9, 9),
                (9, 13),
                (13, 5),
                (13, 9),
            ],
        ),
        (
            Premium::DoubleLetter,
            vec![
                (0, 3),
                (0, 11),
                (2, 6),
                (2, 8),
                (3, 0),
                (3, 7),
                (3, 14),
                (6, 2),
                (6, 6),
                (6, 8),
                (6, 12),
                (7, 3),
                (7, 11),
                (8, 2),
                (8, 6),
                (8, 8),
                (8, 12),
                (11, 0),
                (11, 7),
                (11, 14),
                (12, 6),
                (12, 8),
                (14, 3),
                (14, 11),
            ],
        ),
    ] {
        for (row, column) in squares {
            premiums[row * 15 + column] = premium;
        }
    }
    Ruleset {
        identity: ContentIdentity {
            name: "en-classic-fixture".into(),
            revision: "1".into(),
        },
        dictionary: ContentIdentity {
            name: "scrabble-authored-fixture".into(),
            revision: "1".into(),
        },
        tiles,
        board_size: 15,
        premiums,
        rack_size: 7,
        bingo_bonus: 50,
        exchange_minimum_bag: 7,
        scoreless_turn_limit: 6,
        minimum_players: 2,
        maximum_players: 4,
        board_shape: BoardShape::Square,
        premium_policy: PremiumPolicy::AllWord,
        main_word_length: WordLengthMetric::Tiles,
        time_control: None,
    }
}

/// Small authored German-language fixture, not an official distribution or tournament word list.
#[expect(
    clippy::expect_used,
    reason = "the compiled-in German ruleset is committed content validated by tests"
)]
pub fn german_fixture() -> Ruleset {
    let rules: Ruleset = serde_json::from_str(include_str!("../fixtures/de-mini-ruleset.json"))
        .expect("committed German ruleset parses");
    rules.validate().expect("committed German ruleset is valid");
    rules
}
