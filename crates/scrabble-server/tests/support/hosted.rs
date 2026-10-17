//! Starts the production host (`hosting::serve`) on free loopback ports for tests.
//!
//! Free ports are found by binding port 0 and releasing it, so another process can
//! take one before the server binds it (#147). A startup that fails to bind is retried
//! on fresh ports; any other startup failure or a readiness timeout panics with the
//! server's own error or state instead of a bare timeout.
use scrabble_server::{config::ServerConfig, hosting};
use std::{collections::BTreeMap, error::Error, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
    task::JoinHandle,
};

pub type ServeTask = JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>;

pub struct Hosted {
    pub task: ServeTask,
    pub shutdown: mpsc::Sender<()>,
    pub api_port: u16,
}

const ATTEMPTS: usize = 5;
const READINESS: Duration = Duration::from_secs(20);

fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn bind_conflict(error: &(dyn Error + Send + Sync + 'static)) -> bool {
    let mut source: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(current) = source {
        if current
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::AddrInUse)
        {
            return true;
        }
        source = current.source();
    }
    error.to_string().contains("in use")
}

async fn ready(status_port: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", status_port)).await else {
        return false;
    };
    if stream
        .write_all(b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response).await;
    response.starts_with(b"HTTP/1.1 200")
}

/// `values` is the complete configuration except the three listener ports.
pub async fn start(values: &BTreeMap<String, String>) -> Hosted {
    for attempt in 1..=ATTEMPTS {
        let (port, api_port, status_port) = (free_udp_port(), free_tcp_port(), free_tcp_port());
        let mut values = values.clone();
        values.insert("SCRABBLE_PORT".into(), port.to_string());
        values.insert("SCRABBLE_API_PORT".into(), api_port.to_string());
        values.insert("SCRABBLE_STATUS_PORT".into(), status_port.to_string());
        let config = ServerConfig::from_values(&values).expect("valid test configuration");
        let (shutdown, receiver) = mpsc::channel(1);
        let mut task: ServeTask = tokio::spawn(hosting::serve(config, receiver));
        let started = tokio::time::timeout(READINESS, async {
            loop {
                if task.is_finished() {
                    return false;
                }
                if ready(status_port).await
                    && TcpStream::connect(("127.0.0.1", api_port)).await.is_ok()
                    && !task.is_finished()
                {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
        match started {
            Ok(true) => {
                return Hosted {
                    task,
                    shutdown,
                    api_port,
                };
            }
            Ok(false) => match (&mut task).await.expect("server task panicked") {
                Err(error) if bind_conflict(error.as_ref()) && attempt < ATTEMPTS => {
                    eprintln!("test server lost a reserved port ({error}); retrying");
                }
                Err(error) => panic!("server failed to start: {error}"),
                Ok(()) => panic!("server exited before becoming ready"),
            },
            Err(_) => {
                task.abort();
                panic!(
                    "server not ready within {READINESS:?} (attempt {attempt}); \
                     its task was still running"
                );
            }
        }
    }
    unreachable!("the last attempt returns or panics")
}
