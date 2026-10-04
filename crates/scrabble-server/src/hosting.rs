use crate::{config::ServerConfig, factory::MatchFactory, game_api, games::Games};
use game_server::{
    MatchHostRecoveryConfig, MatchHostStatusConfig, MatchHostWebTransportConfig,
    RejectMatchControlService, prepare_live_match_host_for_recovery,
    serve_prepared_live_match_host_with_status_and_control_and_shutdown,
};
use std::{error::Error, sync::Arc};
use tokio::{net::TcpListener, task::AbortHandle};

struct Forwarder(AbortHandle);
impl Drop for Forwarder {
    fn drop(&mut self) {
        self.0.abort();
    }
}
use tokio::sync::mpsc;

pub async fn serve(
    config: ServerConfig,
    mut shutdown: mpsc::Receiver<()>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = tokio::task::spawn_blocking(move || {
        let factory = MatchFactory::new(&config)?;
        Ok::<_, Box<dyn Error + Send + Sync>>((config, factory))
    })
    .await??;
    let (config, factory) = config;
    let recovery_factory = factory.clone();
    let prepared = prepare_live_match_host_for_recovery(
        config.match_ids,
        move |id| recovery_factory.create(id),
        config.max_matches,
        config.reconnect_grace_ticks,
        MatchHostRecoveryConfig {
            directory: config.recovery_directory,
        },
    )
    .await?;
    let listener = TcpListener::bind(("0.0.0.0", config.api_port)).await?;
    let games = Arc::new(Games::new(
        prepared.host(),
        factory,
        config.route_prefix.clone(),
        config.reconnect_grace_ticks,
    ));
    let (transport_shutdown, transport_receiver) = mpsc::channel(4);
    let forward_sender = transport_shutdown.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(()) = shutdown.recv().await {
            if forward_sender.send(()).await.is_err() {
                break;
            }
        }
    });
    let _forwarder = Forwarder(forwarder.abort_handle());
    let transport = serve_prepared_live_match_host_with_status_and_control_and_shutdown(
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
        transport_receiver,
    );
    tokio::pin!(transport);
    let api = game_api::serve(listener, games, config.board_origin);
    tokio::pin!(api);
    tokio::select! {
        result = &mut transport => result?,
        result = &mut api => {
            let _ = transport_shutdown.send(()).await;
            transport.await?;
            result?;
        },
    }
    Ok(())
}
