//! Authored integration-test data, never a deployment default or tournament dictionary.
use scrabble_game::ruleset::{Premium, TileFace, english_fixture};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

pub const LEGACY_WORDS: &str = include_str!("../../../scrabble-game/fixtures/words.txt");

pub fn write_catalog(
    directory: &Path,
    legacy_words: &str,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    fs::write(directory.join("profile-legacy-words.txt"), legacy_words)?;
    // Every ordered pair supports a real full-rack two-tile opening regardless of the native deal.
    let mut pairs = String::new();
    for first in 'A'..='Z' {
        for second in 'A'..='Z' {
            pairs.push_str(&format!("{first}{second}\n"));
        }
    }
    fs::write(directory.join("profile-pairs.txt"), pairs)?;
    fs::write(directory.join("profile-unicode.txt"), "ÖL\n")?;
    let legacy = english_fixture();
    let mut valued = legacy.tiles.clone();
    for tile in &mut valued {
        if matches!(tile.face, TileFace::Letter { .. }) {
            tile.value *= 2;
        }
        if tile.face == (TileFace::Letter { letter: 'A' }) {
            tile.count += 2;
        }
    }
    let mut compact = vec![Premium::Normal; 49];
    compact[24] = Premium::TripleWord;
    let catalog = serde_json::json!({
        "version":1,
        "wordLists":[
            {"identity":{"name":"acceptance-legacy-words","revision":"1"},"label":"Authored legacy words","file":"profile-legacy-words.txt"},
            {"identity":{"name":"acceptance-pairs","revision":"1"},"label":"Authored two-letter pairs","file":"profile-pairs.txt"},
            {"identity":{"name":"acceptance-unicode","revision":"1"},"label":"Authored unsupported Unicode","file":"profile-unicode.txt"}
        ],
        "tileProfiles":[
            {"identity":{"name":"acceptance-legacy-tiles","revision":"1"},"label":"Classic test tiles","tiles":legacy.tiles},
            {"identity":{"name":"acceptance-valued-tiles","revision":"1"},"label":"Double-value test tiles","tiles":valued}
        ],
        "boardProfiles":[
            {"identity":{"name":"acceptance-classic-board","revision":"1"},"label":"Classic square 15","boardSize":legacy.board_size,"premiums":legacy.premiums},
            {"identity":{"name":"acceptance-compact-board","revision":"1"},"label":"Compact square 7","boardSize":7,"premiums":compact}
        ],
        "defaultSelection":{"wordList":"acceptance-legacy-words","tile":"acceptance-legacy-tiles","board":"acceptance-classic-board"}
    });
    let file = directory.join("profile-catalog.json");
    fs::write(&file, serde_json::to_vec_pretty(&catalog)?)?;
    Ok(file)
}
