//! Composition boundary between Scrabble authority and the reusable serving runtime.
pub mod config;
pub mod dictionary;
pub mod factory;
pub mod game_api;
pub mod games;
pub mod hosting;
pub mod simulation;

pub mod replay;

pub type ErrorResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
