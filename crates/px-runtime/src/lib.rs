mod socks5;
mod upstream;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use px_proto::ClientConfig;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

pub type LogCallback = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone, Default)]
pub struct RuntimeLogger {
    sink: Option<LogCallback>,
}

impl RuntimeLogger {
    pub fn new(sink: Option<LogCallback>) -> Self {
        Self { sink }
    }

    pub fn log<S>(&self, message: S)
    where
        S: Into<String>,
    {
        let message = message.into();
        if let Some(sink) = &self.sink {
            sink(message);
        }
    }
}

pub struct ClientRuntime {
    shutdown_tx: Option<watch::Sender<bool>>,
    task: JoinHandle<()>,
}

impl ClientRuntime {
    pub async fn start(config: ClientConfig, logger: Option<LogCallback>) -> Result<Self> {
        let listener = TcpListener::bind(&config.local_socks_addr)
            .await
            .with_context(|| format!("failed to bind {}", config.local_socks_addr))?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let logger = RuntimeLogger::new(logger);
        let task = tokio::spawn(run_loop(listener, config, logger, shutdown_rx));

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            task,
        })
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub async fn stop(mut self) -> Result<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(true);
        }
        let _ = self.task.await;
        Ok(())
    }

    pub async fn wait(self) -> Result<()> {
        let _ = self.task.await;
        Ok(())
    }
}

async fn run_loop(
    listener: TcpListener,
    config: ClientConfig,
    logger: RuntimeLogger,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    logger.log(format!(
        "客户端开始监听，本地 SOCKS5: {}，服务端: {}",
        config.local_socks_addr, config.server_addr
    ));

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        let config = config.clone();
                        let logger = logger.clone();
                        tokio::spawn(async move {
                            if let Err(error) = socks5::handle_client(stream, peer_addr, config).await {
                                let message = format!("客户端会话失败，来源 {peer_addr}: {error}");
                                logger.log(message.clone());
                                error!(peer = %peer_addr, error = %error, "client session failed");
                            }
                        });
                    }
                    Err(error) => {
                        let message = format!("接受本地连接失败: {error}");
                        logger.log(message.clone());
                        error!(error = %error, "accept failed");
                        break;
                    }
                }
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    logger.log("客户端已停止。");
                    break;
                }
            }
        }
    }
}

pub async fn run_forever(config: ClientConfig, logger: Option<LogCallback>) -> Result<()> {
    let runtime = ClientRuntime::start(config, logger).await?;
    runtime.wait().await
}

pub fn config_path_from_args() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if let Some(value) = args.next() {
                return PathBuf::from(value);
            }
        }
    }
    PathBuf::from("config/client.toml")
}

pub fn init_tracing(level: &str) {
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    info!(log_level = %level, "px-runtime tracing initialized");
}
