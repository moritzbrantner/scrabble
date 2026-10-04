//! Deterministic Scrabble domain and versioned wire contracts.
pub mod commit;
pub mod dictionary;
pub mod identity;
pub mod protocol;
pub mod ruleset;
pub const APPLICATION_NAME: &str = "Scrabble";
pub mod placement;
pub mod scoring;
pub mod session;
pub mod state;
