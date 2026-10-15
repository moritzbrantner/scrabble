//! Pure structural validation. Dictionary acceptance and scoring are separate boundaries.
use crate::identity::{Coordinate, PlayerId};
use crate::protocol::{Phase, Placement};
use crate::ruleset::{BoardShape, WordLengthMetric};
use crate::state::{GameState, StateError};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlacementError {
    Storage(StateError),
    Empty,
    TooManyTiles,
    NotLine,
    Gap,
    CenterRequired,
    Disconnected,
    NoWord,
}
impl fmt::Display for PlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for PlacementError {}
impl From<StateError> for PlacementError {
    fn from(error: StateError) -> Self {
        Self::Storage(error)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Horizontal,
    Vertical,
    /// Third hexagonal axis, step `(+1, −1)`; never used on square boards.
    Diagonal,
}
impl Direction {
    const fn step(self) -> (i16, i16) {
        match self {
            Self::Horizontal => (0, 1),
            Self::Vertical => (1, 0),
            Self::Diagonal => (1, -1),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WordTile {
    coordinate: Coordinate,
    /// The tile's whole spelled face; a multi-letter tile contributes all its letters.
    letter: String,
    value: u16,
    is_blank: bool,
    is_new: bool,
}
impl WordTile {
    pub const fn coordinate(&self) -> Coordinate {
        self.coordinate
    }
    /// The whole spelled face of this cell; never truncated to one character.
    pub fn letter(&self) -> &str {
        &self.letter
    }
    pub const fn value(&self) -> u16 {
        self.value
    }
    pub const fn is_blank(&self) -> bool {
        self.is_blank
    }
    pub const fn is_new(&self) -> bool {
        self.is_new
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Word {
    direction: Direction,
    tiles: Vec<WordTile>,
}
impl Word {
    pub const fn direction(&self) -> Direction {
        self.direction
    }
    pub fn tiles(&self) -> &[WordTile] {
        &self.tiles
    }
    pub fn text(&self) -> String {
        self.tiles.iter().map(|tile| tile.letter.as_str()).collect()
    }
}
/// Borrows the validated state, so a caller cannot mutate the board while using its derived words.
pub struct ValidatedMove<'a> {
    state: &'a GameState,
    placements: Vec<Placement>,
    words: Vec<Word>,
}
impl ValidatedMove<'_> {
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }
    /// Main word first, followed by cross words in board order; each affected word occurs once.
    pub fn words(&self) -> &[Word] {
        &self.words
    }
    pub fn ruleset(&self) -> &crate::ruleset::Ruleset {
        self.state.ruleset()
    }
}

pub fn validate<'a>(
    state: &'a GameState,
    player: PlayerId,
    placements: &[Placement],
) -> Result<ValidatedMove<'a>, PlacementError> {
    if !matches!(state.phase(), Phase::Playing { .. }) {
        return Err(StateError::WrongPhase.into());
    }
    if placements.is_empty() {
        return Err(PlacementError::Empty);
    }
    if placements.len() > usize::from(state.ruleset().rack_size) {
        return Err(PlacementError::TooManyTiles);
    }
    let previews = state.preview_tiles(player, placements)?;
    let rack = state.rack(player).ok_or(StateError::UnknownPlayer)?;
    let mut proposed = BTreeMap::new();
    for (placement, preview) in placements.iter().zip(previews) {
        let tile = rack
            .iter()
            .find(|tile| tile.id() == placement.tile_id)
            .ok_or(StateError::TileNotOwned)?;
        proposed.insert(
            (placement.coordinate.row(), placement.coordinate.column()),
            WordTile {
                coordinate: placement.coordinate,
                letter: preview.letter,
                value: tile.value(),
                is_blank: preview.is_blank,
                is_new: true,
            },
        );
    }
    let rules = state.ruleset();
    let hex = rules.board_shape == BoardShape::Hexagon;
    let axes: &[Direction] = if hex {
        &[
            Direction::Horizontal,
            Direction::Vertical,
            Direction::Diagonal,
        ]
    } else {
        &[Direction::Horizontal, Direction::Vertical]
    };
    let first = placements[0].coordinate;
    let on_line = |direction: Direction| {
        placements.iter().all(|tile| match direction {
            Direction::Horizontal => tile.coordinate.row() == first.row(),
            Direction::Vertical => tile.coordinate.column() == first.column(),
            Direction::Diagonal => {
                u16::from(tile.coordinate.row()) + u16::from(tile.coordinate.column())
                    == u16::from(first.row()) + u16::from(first.column())
            }
        })
    };
    let line = axes.iter().copied().find(|&direction| on_line(direction));
    let Some(line) = line else {
        return Err(PlacementError::NotLine);
    };
    let size = rules.board_size;
    let at = |row: i16, column: i16| -> Option<WordTile> {
        let row = u8::try_from(row).ok()?;
        let column = u8::try_from(column).ok()?;
        if !rules.contains(row, column) {
            return None;
        }
        let coordinate = Coordinate::new(row, column).ok()?;
        proposed.get(&(row, column)).cloned().or_else(|| {
            state.board_tile(coordinate).map(|tile| WordTile {
                coordinate,
                letter: tile.letter().to_owned(),
                value: tile.tile().value(),
                is_blank: tile.is_blank(),
                is_new: false,
            })
        })
    };
    let word = |coordinate: Coordinate, direction: Direction| -> Word {
        let (dr, dc) = direction.step();
        let (mut row, mut column) = (i16::from(coordinate.row()), i16::from(coordinate.column()));
        while at(row - dr, column - dc).is_some() {
            row -= dr;
            column -= dc;
        }
        let mut tiles = Vec::new();
        while let Some(tile) = at(row, column) {
            tiles.push(tile);
            row += dr;
            column += dc;
        }
        Word { direction, tiles }
    };
    let direction = if placements.len() > 1 {
        line
    } else if hex {
        // Decision 1.15: longest formed word; ties follow the fixed axis order.
        let length = |word: &Word| match rules.main_word_length {
            WordLengthMetric::Tiles => word.tiles.len(),
            WordLengthMetric::Letters => word.text().chars().count(),
        };
        let mut best = (Direction::Horizontal, 0);
        for &axis in axes {
            let candidate = word(first, axis);
            if candidate.tiles.len() > 1 && length(&candidate) > best.1 {
                best = (axis, length(&candidate));
            }
        }
        best.0
    } else if word(first, Direction::Horizontal).tiles.len() > 1 {
        Direction::Horizontal
    } else {
        Direction::Vertical
    };
    let main = word(first, direction);
    if main.tiles.iter().filter(|tile| tile.is_new).count() != placements.len() {
        return Err(PlacementError::Gap);
    }
    let empty = state.board_is_empty();
    if empty && !proposed.contains_key(&(size / 2, size / 2)) {
        return Err(PlacementError::CenterRequired);
    }
    let mut words = Vec::new();
    if main.tiles.len() > 1 {
        words.push(main);
    }
    for tile in proposed.values() {
        for &axis in axes.iter().filter(|&&axis| axis != direction) {
            let cross = word(tile.coordinate, axis);
            if cross.tiles.len() > 1 {
                words.push(cross);
            }
        }
    }
    if words.is_empty() {
        return Err(PlacementError::NoWord);
    }
    if !empty
        && !words
            .iter()
            .any(|word| word.tiles.iter().any(|tile| !tile.is_new))
    {
        return Err(PlacementError::Disconnected);
    }
    let mut ordered = placements.to_vec();
    ordered.sort_by_key(|placement| (placement.coordinate.row(), placement.coordinate.column()));
    Ok(ValidatedMove {
        state,
        placements: ordered,
        words,
    })
}
