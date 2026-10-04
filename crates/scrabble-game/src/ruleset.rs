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
}
impl Ruleset {
    pub fn validate(&self) -> Result<(), ContractError> {
        for identity in [&self.identity, &self.dictionary] {
            if identity.name.is_empty()
                || identity.name.len() > 64
                || !identity
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                || identity.revision.is_empty()
                || identity.revision.len() > 192
                || !identity
                    .revision
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+:".contains(&byte))
            {
                return Err(ContractError(
                    "content identity requires bounded identifiers and revisions".into(),
                ));
            }
        }
        if self.board_size == 0
            || self.board_size > 15
            || self.board_size.is_multiple_of(2)
            || self.premiums.len() != usize::from(self.board_size).pow(2)
            || self.rack_size == 0
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
        if self.tiles.len() > 64 {
            return Err(ContractError(
                "tile alphabet exceeds supported bound".into(),
            ));
        }
        let mut letters = BTreeSet::new();
        let mut blank = false;
        let mut total = 0_u32;
        for tile in &self.tiles {
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
        if letters.is_empty()
            || total < u32::from(self.maximum_players) * u32::from(self.rack_size)
            || total > 200
        {
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
    }
}

/// Small authored German-language fixture, not an official distribution or tournament word list.
pub fn german_fixture() -> Ruleset {
    let rules: Ruleset = serde_json::from_str(include_str!("../fixtures/de-mini-ruleset.json"))
        .expect("committed German ruleset parses");
    rules.validate().expect("committed German ruleset is valid");
    rules
}
