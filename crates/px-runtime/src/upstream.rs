use std::fs::File;
use std::io::BufReader;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use px_proto::{ClientConfig, ConnectRequest};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig as RustlsClientConfig, RootCertStore};
use socket2::{SockRef, TcpKeepalive};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

pub async fn connect(
    config: &ClientConfig,
    request: &ConnectRequest,
) -> Result<TlsStream<TcpStream>> {
    let server_addr: SocketAddr = config
        .server_addr
        .parse()
        .context("server_addr must be IP:port")?;
    let tcp = timeout(
        Duration::from_millis(config.connect_timeout_ms),
        TcpStream::connect(server_addr),
    )
    .await
    .context("tcp connect timeout")??;
    apply_socket_options(&tcp)?;

    let tls_config = build_tls_config(&config.server_cert_path)?;
    let connector = TlsConnector::from(Arc::new(tls_config));
    let server_name = ServerName::IpAddress(match server_addr.ip() {
        IpAddr::V4(ip) => ip.into(),
        IpAddr::V6(ip) => ip.into(),
    });
    let mut tls_stream = connector
        .connect(server_name, tcp)
        .await
        .context("tls connect failed")?;
    request.write_to(&mut tls_stream).await?;
    Ok(tls_stream)
}

fn build_tls_config(path: &str) -> Result<RustlsClientConfig> {
    let certs = load_certs(path)?;
    let mut roots = RootCertStore::empty();
    for cert in certs {
        roots.add(cert)?;
    }
    Ok(RustlsClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path).with_context(|| format!("failed to open pinned cert {path}"))?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to parse pinned cert")?;
    Ok(certs)
}

fn apply_socket_options(stream: &TcpStream) -> Result<()> {
    stream.set_nodelay(true)?;
    let sock = SockRef::from(stream);
    let keepalive = TcpKeepalive::new().with_time(Duration::from_secs(30));
    sock.set_tcp_keepalive(&keepalive)?;
    Ok(())
}
