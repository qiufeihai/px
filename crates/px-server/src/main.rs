mod session;
mod target;

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use px_proto::{load_server_config, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig as RustlsServerConfig;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = config_path_from_args();
    let config = load_server_config(&config_path)?;
    init_tracing(&config.log_level);

    let tls_acceptor = TlsAcceptor::from(Arc::new(build_tls_config(&config)?));
    let listener = TcpListener::bind(&config.listen_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.listen_addr))?;

    info!(listen_addr = %config.listen_addr, "px-server listening");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let tls_acceptor = tls_acceptor.clone();
        let config = config.clone();

        tokio::spawn(async move {
            if let Err(error) = session::handle_client(stream, peer_addr, tls_acceptor, config).await
            {
                error!(peer = %peer_addr, error = %error, "session failed");
            }
        });
    }
}

fn config_path_from_args() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if let Some(value) = args.next() {
                return PathBuf::from(value);
            }
        }
    }
    PathBuf::from("config/server.toml")
}

fn init_tracing(level: &str) {
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn build_tls_config(config: &ServerConfig) -> Result<RustlsServerConfig> {
    let certs = load_certs(&config.tls_cert_path)?;
    let key = load_private_key(&config.tls_key_path)?;
    let server_config = RustlsServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(server_config)
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path).with_context(|| format!("failed to open cert file {path}"))?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to parse certs")?;
    Ok(certs)
}

fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let file = File::open(path).with_context(|| format!("failed to open key file {path}"))?;
    let mut reader = BufReader::new(file);
    if let Some(key) = rustls_pemfile::pkcs8_private_keys(&mut reader).next() {
        return Ok(PrivateKeyDer::Pkcs8(key?));
    }

    let file = File::open(path).with_context(|| format!("failed to reopen key file {path}"))?;
    let mut reader = BufReader::new(file);
    if let Some(key) = rustls_pemfile::rsa_private_keys(&mut reader).next() {
        return Ok(PrivateKeyDer::Pkcs1(key?));
    }

    anyhow::bail!("no private key found in {path}")
}
