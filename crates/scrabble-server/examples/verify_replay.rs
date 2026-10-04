//! Read-only inspection; no serving, token output, recovery consumption, or alternate state store.
use game_server::{MatchId, RecoveryImage};
use scrabble_server::{
    ErrorResult, config::ServerConfig, factory::MatchFactory, replay::inspect_replay,
};
use std::{env, path::Path};

fn main() -> ErrorResult<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: verify_replay MATCH_ID RECOVERY_IMAGE (with original SCRABBLE_* initialization settings)".into());
    }
    let id = MatchId::new(
        args[0]
            .to_str()
            .ok_or("match identity must be UTF-8")?
            .to_owned(),
    )?;
    let factory = MatchFactory::new(&ServerConfig::from_env()?)?;
    let image = RecoveryImage::read_file(Path::new(&args[1]))?;
    let report = inspect_replay(&factory, &id, &image)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
