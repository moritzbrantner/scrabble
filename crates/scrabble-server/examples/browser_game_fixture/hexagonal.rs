//! Independent #96 data and real native creation-input selection. No board/rack grants.
use super::{Opening, multi_letter};
use ring::hmac;
use scrabble_game::{
    identity::PlayerId,
    ruleset::{BoardShape, Premium, TileFace, english_fixture},
};
use scrabble_server::{config::ServerConfig, factory::MatchFactory};
use std::{collections::BTreeMap, error::Error, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub fn add_sources(directory: &Path, catalog: &Path) -> Result<()> {
    std::fs::write(
        directory.join("profile-hex-words.txt"),
        "SCHA\nAA\nASCH\nAAA\nAAAA\nASCHCH\nAACH\nACH\nASCHA\n",
    )?;
    let mut document: serde_json::Value = serde_json::from_slice(&std::fs::read(catalog)?)?;
    document["wordLists"]
        .as_array_mut()
        .ok_or("word-list catalog")?
        .push(serde_json::json!({
            "identity":{"name":"acceptance-hex-words","revision":"1"},
            "label":"Hex trace words","file":"profile-hex-words.txt"
        }));
    let mut premiums = Vec::new();
    for row in 0..7 {
        for column in 0..7 {
            if (3..=9).contains(&(row + column)) {
                premiums.push(if (row, column) == (2, 3) {
                    Premium::DoubleWord
                } else {
                    Premium::Normal
                });
            }
        }
    }
    for (policy, label) in [
        ("all_word", "all-word"),
        ("main_word_only", "main-word-only"),
    ] {
        for metric in ["tiles", "letters"] {
            document["boardProfiles"]
                .as_array_mut()
                .ok_or("board catalog")?
                .push(serde_json::json!({
                    "identity":{"name":format!("acceptance-hex-{policy}-{metric}"),"revision":"1"},
                    "label":format!("Hex {label} ({metric})"), "boardSize":7,"boardShape":"hexagon",
                    "premiumPolicy":policy,"mainWordLength":metric,"premiums":premiums
                }));
        }
    }
    std::fs::write(catalog, serde_json::to_vec_pretty(&document)?)?;
    Ok(())
}

pub fn opening(directory: &Path) -> Result<Opening> {
    // Reuse #98's native-v2 derivation/deal guard rather than silently trusting a
    // second randomization algorithm. Only the timestamped creation inputs leave here.
    let _ = multi_letter::opening(directory, None)?;
    let values = serde_json::from_slice::<BTreeMap<String, String>>(&std::fs::read(
        directory.join("fixture-config.json"),
    )?)?;
    let config = ServerConfig::from_values(&values)?;
    let _ = MatchFactory::new(&config)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &std::fs::read(&config.seed_file)?);
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    // Deal order depends on the native creation identity and selected physical
    // tile bag, not board premiums/metric. The browser verifies the actual racks.
    let mut rules = english_fixture();
    rules.tiles = serde_json::from_value(multi_letter::tiles())?;
    rules.board_shape = BoardShape::Hexagon;
    rules.board_size = 7;
    rules.premiums = vec![Premium::Normal; 37];
    for nonce in 0..200_000_u32 {
        let request_id = format!("{nonce:032x}");
        let id = multi_letter::creation_id(&key, b"scrabble/create/v3\0", "p", &request_id, at)?;
        let state = multi_letter::deal(&id, &rules, &key)?;
        let count = |player, face| {
            state
                .rack(PlayerId::new(player))
                .unwrap()
                .iter()
                .filter(|tile| tile.face() == face)
                .count()
        };
        if count(2, TileFace::Letter { letter: 'A' }) >= 2
            && count(
                2,
                TileFace::Letters {
                    letters: "CH".into(),
                },
            ) >= 1
            && count(
                2,
                TileFace::Letters {
                    letters: "SCH".into(),
                },
            ) >= 1
            && count(3, TileFace::Letter { letter: 'A' }) >= 3
            && count(3, TileFace::Blank {}) >= 1
        {
            return Ok(Opening {
                request_id,
                requested_at: at,
            });
        }
    }
    Err("no real creation deal supports the authored six-turn hex trace".into())
}

pub fn replay(directory: &Path, match_id: &str) -> Result<scrabble_server::replay::ReplayReport> {
    let values = serde_json::from_slice::<BTreeMap<String, String>>(&std::fs::read(
        directory.join("fixture-config.json"),
    )?)?;
    let config = ServerConfig::from_values(&values)?;
    let factory = MatchFactory::new(&config)?;
    let id = game_server::MatchId::new(match_id.to_owned())?;
    for entry in std::fs::read_dir(directory.join("recovery"))? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(image) = game_server::RecoveryImage::read_file(&path)
            && let Ok(report) = scrabble_server::replay::inspect_replay(&factory, &id, &image)
        {
            return Ok(report);
        }
    }
    Err("no matching verifiable recovery image exists yet".into())
}
