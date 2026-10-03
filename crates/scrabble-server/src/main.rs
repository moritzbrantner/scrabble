use scrabble_server::{config::ServerConfig, hosting::serve};
use std::error::Error;
use tokio::{sync::mpsc, task::JoinHandle};

#[cfg(unix)]
fn signals(sender: mpsc::Sender<()>) -> std::io::Result<JoinHandle<std::io::Result<()>>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    Ok(tokio::spawn(async move {
        loop {
            let received = tokio::select! { received = terminate.recv() => received, received = interrupt.recv() => received };
            if received.is_none() || sender.send(()).await.is_err() {
                return Ok(());
            }
        }
    }))
}
#[cfg(not(unix))]
fn signals(sender: mpsc::Sender<()>) -> std::io::Result<JoinHandle<std::io::Result<()>>> {
    Ok(tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await?;
            if sender.send(()).await.is_err() {
                return Ok(());
            }
        }
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = ServerConfig::from_env()?;
    let (sender, receiver) = mpsc::channel(4);
    let signal_task = signals(sender)?;
    let result = serve(config, receiver).await;
    signal_task.abort();
    match signal_task.await {
        Ok(result) => result?,
        Err(error) if error.is_cancelled() => {}
        Err(error) => return Err(error.into()),
    }
    result
}
