use crate::{config::ServerConfig, factory::MatchFactory};
use game_server::{
    MatchHostRecoveryConfig, MatchHostStatusConfig, MatchHostWebTransportConfig,
    RejectMatchControlService, prepare_live_match_host_for_recovery,
    serve_prepared_live_match_host_with_status_and_control_and_shutdown,
};
use std::error::Error;
use tokio::sync::mpsc;

pub async fn serve(
    config: ServerConfig,
    shutdown: mpsc::Receiver<()>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = tokio::task::spawn_blocking(move || {
        let factory = MatchFactory::new(&config)?;
        Ok::<_, Box<dyn Error + Send + Sync>>((config, factory))
    })
    .await??;
    let (config, factory) = config;
    let prepared = prepare_live_match_host_for_recovery(
        config.match_ids,
        move |id| factory.create(id),
        config.max_matches,
        config.reconnect_grace_ticks,
        MatchHostRecoveryConfig {
            directory: config.recovery_directory,
        },
    )
    .await?;
    serve_prepared_live_match_host_with_status_and_control_and_shutdown(
        prepared,
        RejectMatchControlService,
        MatchHostWebTransportConfig {
            port: config.port,
            certificate_pem: config.certificate,
            private_key_pem: config.private_key,
            route_prefix: config.route_prefix,
            drain_grace: config.drain_grace,
        },
        MatchHostStatusConfig {
            port: config.status_port,
        },
        shutdown,
    )
    .await?;
    Ok(())
}
