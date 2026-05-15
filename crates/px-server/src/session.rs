use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use px_proto::{ConnectResponse, StatusCode, TargetAddr};
use socket2::{SockRef, TcpKeepalive};
use tokio::io::copy_bidirectional;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, warn};

use crate::target::connect_target;

pub async fn handle_client(
    stream: TcpStream,
    peer_addr: SocketAddr,
    tls_acceptor: TlsAcceptor,
    config: px_proto::ServerConfig,
) -> Result<()> {
    apply_socket_options(&stream)?;
    let mut tls_stream = tls_acceptor.accept(stream).await.context("tls accept failed")?;
    let request = match px_proto::ConnectRequest::read_from(&mut tls_stream).await {
        Ok(request) => request,
        Err(error) => {
            warn!(peer = %peer_addr, error = %error, "invalid connect request");
            let _ = ConnectResponse {
                status: StatusCode::BadRequest,
                reason: 0,
            }
            .write_to(&mut tls_stream)
            .await;
            return Err(error.into());
        }
    };

    debug!(peer = %peer_addr, port = request.port, "connect request received");
    let timeout_ms = Duration::from_millis(config.connect_timeout_ms);
    let mut target_stream = match timeout(timeout_ms, connect_target(&request)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            warn!(
                peer = %peer_addr,
                target = %format_target(&request),
                error = %error,
                "target connect failed"
            );
            let _ = ConnectResponse {
                status: StatusCode::TargetConnectFailed,
                reason: 0,
            }
            .write_to(&mut tls_stream)
            .await;
            return Err(error);
        }
        Err(_) => {
            warn!(
                peer = %peer_addr,
                target = %format_target(&request),
                timeout_ms = config.connect_timeout_ms,
                "target connect timeout"
            );
            let _ = ConnectResponse {
                status: StatusCode::Timeout,
                reason: 0,
            }
            .write_to(&mut tls_stream)
            .await;
            anyhow::bail!("target connect timeout");
        }
    };

    apply_socket_options(&target_stream)?;
    ConnectResponse {
        status: StatusCode::Ok,
        reason: 0,
    }
    .write_to(&mut tls_stream)
    .await?;

    debug!(peer = %peer_addr, port = request.port, "relay started");
    let (upstream_bytes, downstream_bytes) = relay(&mut tls_stream, &mut target_stream).await?;
    debug!(peer = %peer_addr, upstream_bytes, downstream_bytes, "relay finished");
    Ok(())
}

async fn relay(client: &mut TlsStream<TcpStream>, target: &mut TcpStream) -> Result<(u64, u64)> {
    Ok(copy_bidirectional(client, target).await?)
}

fn apply_socket_options(stream: &TcpStream) -> Result<()> {
    stream.set_nodelay(true)?;
    let sock = SockRef::from(stream);
    let keepalive = TcpKeepalive::new().with_time(Duration::from_secs(30));
    sock.set_tcp_keepalive(&keepalive)?;
    Ok(())
}

fn format_target(request: &px_proto::ConnectRequest) -> String {
    match &request.target {
        TargetAddr::Ip(ip) => format!("{ip}:{}", request.port),
        TargetAddr::Domain(domain) => format!("{domain}:{}", request.port),
    }
}
