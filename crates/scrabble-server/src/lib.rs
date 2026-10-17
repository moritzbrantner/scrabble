//! Composition boundary between Scrabble authority and the reusable serving runtime.
pub mod config;
pub mod dictionary;
pub mod factory;
pub mod game_api;
pub mod games;
pub mod hosting;
pub mod layout_library;
pub mod profiles;
pub mod simulation;
pub mod tile_profile_library;
pub mod word_list_library;

pub mod replay;

pub type ErrorResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

mod configuration_store;
mod profile_catalog;
