//! Pure structural validation. Dictionary acceptance and scoring are separate boundaries.
use crate::identity::{Coordinate, PlayerId};
use crate::protocol::{Phase, Placement};
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
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WordTile {
    coordinate: Coordinate,
    letter: char,
    value: u16,
    is_blank: bool,
    is_new: bool,
}
impl WordTile {
    pub const fn coordinate(self) -> Coordinate {
        self.coordinate
    }
    pub const fn letter(self) -> char {
        self.letter
    }
    pub const fn value(self) -> u16 {
        self.value
    }
    pub const fn is_blank(self) -> bool {
        self.is_blank
    }
    pub const fn is_new(self) -> bool {
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
        self.tiles.iter().map(|tile| tile.letter).collect()
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
    let first = placements[0].coordinate;
    let horizontal = placements
        .iter()
        .all(|tile| tile.coordinate.row() == first.row());
    let vertical = placements
        .iter()
        .all(|tile| tile.coordinate.column() == first.column());
    if !horizontal && !vertical {
        return Err(PlacementError::NotLine);
    }
    let size = state.ruleset().board_size;
    let at = |row: u8, column: u8| -> Option<WordTile> {
        let coordinate = Coordinate::new(row, column).ok()?;
        proposed.get(&(row, column)).copied().or_else(|| {
            state.board_tile(coordinate).map(|tile| WordTile {
                coordinate,
                letter: tile.letter(),
                value: tile.tile().value(),
                is_blank: tile.is_blank(),
                is_new: false,
            })
        })
    };
    let word = |coordinate: Coordinate, direction: Direction| -> Word {
        let (dr, dc) = match direction {
            Direction::Horizontal => (0, 1),
            Direction::Vertical => (1, 0),
        };
        let (mut row, mut column) = (coordinate.row(), coordinate.column());
        while row >= dr && column >= dc && at(row - dr, column - dc).is_some() {
            row -= dr;
            column -= dc;
        }
        let mut tiles = Vec::new();
        while row < size && column < size {
            let Some(tile) = at(row, column) else {
                break;
            };
            tiles.push(tile);
            row += dr;
            column += dc;
        }
        Word { direction, tiles }
    };
    let direction = if placements.len() == 1 {
        if word(first, Direction::Horizontal).tiles.len() > 1 {
            Direction::Horizontal
        } else {
            Direction::Vertical
        }
    } else if horizontal {
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
    let perpendicular = match direction {
        Direction::Horizontal => Direction::Vertical,
        Direction::Vertical => Direction::Horizontal,
    };
    for tile in proposed.values() {
        let cross = word(tile.coordinate, perpendicular);
        if cross.tiles.len() > 1 {
            words.push(cross);
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
