use game_server::GameSimulation;
use scrabble_server::{config::ServerConfig, factory::create_matches};
use std::{collections::BTreeMap, fs};

fn values() -> BTreeMap<String, String> {
    BTreeMap::new()
}
fn temporary_config(directory: &std::path::Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "SCRABBLE_SEED_FILE".into(),
            directory.join("seed").to_str().unwrap().into(),
        ),
        (
            "SCRABBLE_RECOVERY_DIR".into(),
            directory.join("recovery").to_str().unwrap().into(),
        ),
    ])
}

#[test]
fn local_defaults_and_invalid_configuration_are_explicit() {
    let defaults = ServerConfig::from_values(&values()).unwrap();
    assert_eq!(defaults.port, 4433);
    assert_eq!(defaults.status_port, 8080);
    assert_eq!(defaults.match_ids.len(), 2);
    assert!(defaults.allowed_origins.is_none());
    for (key, value) in [
        ("SCRABBLE_PORT", "0"),
        ("SCRABBLE_STATUS_PORT", "65536"),
        ("SCRABBLE_PORT", "abc"),
        ("SCRABBLE_PORT", " 4433"),
        ("SCRABBLE_MODE", "prod"),
        ("SCRABBLE_MAX_MATCHES", "1"),
        ("SCRABBLE_MATCH_IDS", "one,one"),
        ("SCRABBLE_MATCH_IDS", "one,../two"),
        ("SCRABBLE_ROUTE_PREFIX", "/game/"),
        ("SCRABBLE_ROUTE_PREFIX", "/%2e/game"),
        ("SCRABBLE_TEST_SEED", "PRIVATE-BAD-VALUE"),
        ("SCRABBLE_RECOVERY_DIR", ""),
        ("SCRABBLE_POTR", "1234"),
        ("SCRABBLE_API_PORT", "8080"),
        ("SCRABBLE_BOARD_ORIGIN", "https://board.example/path"),
        ("SCRABBLE_BOARD_ORIGIN", "https://user:secret@board.example"),
        ("SCRABBLE_BOARD_ORIGIN", "*"),
    ] {
        let error = ServerConfig::from_values(&BTreeMap::from([(key.into(), value.into())]))
            .err()
            .unwrap();
        assert!(!error.to_string().contains("PRIVATE-BAD-VALUE"));
    }
    assert!(
        ServerConfig::from_values(&BTreeMap::from([("SCRABBLE_MATCH_IDS".into(), "".into())]))
            .unwrap()
            .match_ids
            .is_empty()
    );
    let mut config = values();
    config.insert(
        "SCRABBLE_SEED_FILE".into(),
        ".local/server/recovery/seed".into(),
    );
    assert!(ServerConfig::from_values(&config).is_err());
}

#[test]
fn production_requires_private_persistent_initialization_and_explicit_tls() {
    let mut config = BTreeMap::from([("SCRABBLE_MODE".into(), "production".into())]);
    assert!(ServerConfig::from_values(&config).is_err());
    for (key, value) in [
        ("SCRABBLE_CERT_PEM", "/etc/scrabble/cert.pem"),
        ("SCRABBLE_KEY_PEM", "/etc/scrabble/key.pem"),
        ("SCRABBLE_SEED_FILE", "/var/lib/scrabble/seed"),
        ("SCRABBLE_RECOVERY_DIR", "/var/lib/scrabble/recovery"),
        ("SCRABBLE_MATCH_IDS", "one,two"),
        ("SCRABBLE_BOARD_ORIGIN", "https://board.example"),
    ] {
        config.insert(key.into(), value.into());
    }
    let production = ServerConfig::from_values(&config).unwrap();
    let allowed = production.allowed_origins.unwrap();
    assert!(allowed.allows(Some("https://board.example")));
    assert!(!allowed.allows(None));
    assert!(!allowed.allows(Some("https://other.example")));
    config.insert("SCRABBLE_TEST_SEED".into(), "07".repeat(32));
    assert!(ServerConfig::from_values(&config).is_err());
    config.remove("SCRABBLE_TEST_SEED");
    config.insert(
        "SCRABBLE_DICTIONARY_FILE".into(),
        "/var/lib/scrabble/words.txt".into(),
    );
    config.insert("SCRABBLE_DICTIONARY_NAME".into(), "deployment-list".into());
    config.insert("SCRABBLE_DICTIONARY_REVISION".into(), "2026.1".into());
    assert!(ServerConfig::from_values(&config).is_ok());
    config.insert(
        "SCRABBLE_DICTIONARY_FILE".into(),
        "relative/words.txt".into(),
    );
    assert!(ServerConfig::from_values(&config).is_err());
    config.insert(
        "SCRABBLE_DICTIONARY_FILE".into(),
        "/var/lib/scrabble/words.txt".into(),
    );
    config.insert("SCRABBLE_SEED_FILE".into(), "relative/seed".into());
    assert!(ServerConfig::from_values(&config).is_err());
}

#[test]
fn private_seed_and_match_initialization_survive_restart_and_configuration_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut values = temporary_config(directory.path());
    values.insert("SCRABBLE_TEST_SEED".into(), "07".repeat(32));
    let config = ServerConfig::from_values(&values).unwrap();
    let first = create_matches(&config).unwrap();
    let seed_before = fs::read(&config.seed_file).unwrap();
    assert_eq!(seed_before, [7; 32]);
    values.insert("SCRABBLE_MATCH_IDS".into(), "table-2,table-1".into());
    let reversed = ServerConfig::from_values(&values).unwrap();
    let second = create_matches(&reversed).unwrap();
    for ((first_id, first), (second_id, second)) in first.iter().zip(&second) {
        assert_eq!(first_id, second_id);
        assert_eq!(first.snapshot().unwrap(), second.snapshot().unwrap());
    }
    assert_ne!(
        first[0].1.snapshot().unwrap(),
        first[1].1.snapshot().unwrap()
    );
    assert_eq!(fs::read(&config.seed_file).unwrap(), seed_before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&config.seed_file)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    values.insert("SCRABBLE_TEST_SEED".into(), "08".repeat(32));
    assert!(create_matches(&ServerConfig::from_values(&values).unwrap()).is_err());
    assert_eq!(fs::read(&config.seed_file).unwrap(), seed_before);
}

#[test]
fn lost_corrupt_or_exposed_initialization_fails_without_replacing_authority() {
    let directory = tempfile::tempdir().unwrap();
    let config = ServerConfig::from_values(&temporary_config(directory.path())).unwrap();
    fs::create_dir(&config.recovery_directory).unwrap();
    assert!(create_matches(&config).is_err());
    assert!(!config.seed_file.exists());
    fs::remove_dir(&config.recovery_directory).unwrap();
    fs::write(&config.seed_file, b"invalid").unwrap();
    assert!(create_matches(&config).is_err());
    assert_eq!(fs::read(&config.seed_file).unwrap(), b"invalid");
    fs::remove_file(&config.seed_file).unwrap();
    let matches = create_matches(&config).unwrap();
    let original = fs::read(&config.seed_file).unwrap();
    assert_eq!(original.len(), 32);
    assert_eq!(
        create_matches(&config).unwrap()[0].1.snapshot().unwrap(),
        matches[0].1.snapshot().unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&config.seed_file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(create_matches(&config).is_err());
        assert_eq!(fs::read(&config.seed_file).unwrap(), original);
    }
}
