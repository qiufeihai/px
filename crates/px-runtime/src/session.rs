use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::Result;
use px_proto::{ClientConfig, ConnectRequest, ConnectResponse, StatusCode, TargetAddr};
use tokio::io::{copy_bidirectional, AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tracing::info;

use crate::upstream::UpstreamConnector;
use crate::RuntimeLogger;

pub type UpstreamStream = TlsStream<TcpStream>;

#[derive(Debug)]
pub enum OpenStreamError {
    Connect(anyhow::Error),
    Timeout(anyhow::Error),
    ResponseRead(anyhow::Error),
    Refused(StatusCode),
}

pub async fn open_upstream_stream(
    request: &ConnectRequest,
    config: &ClientConfig,
    upstream: &Arc<UpstreamConnector>,
) -> std::result::Result<UpstreamStream, OpenStreamError> {
    let timeout_ms = Duration::from_millis(config.connect_timeout_ms);

    let mut upstream = match tokio::time::timeout(timeout_ms, upstream.connect(request)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => return Err(OpenStreamError::Connect(error)),
        Err(error) => return Err(OpenStreamError::Timeout(error.into())),
    };

    let response = match ConnectResponse::read_from(&mut upstream).await {
        Ok(response) => response,
        Err(error) => return Err(OpenStreamError::ResponseRead(error.into())),
    };

    match response.status {
        StatusCode::Ok => Ok(upstream),
        status => Err(OpenStreamError::Refused(status)),
    }
}

pub async fn relay_bidirectional(
    inbound: &mut TcpStream,
    upstream: &mut UpstreamStream,
) -> Result<(u64, u64)> {
    let copied = copy_bidirectional(inbound, upstream).await?;
    Ok(copied)
}

pub async fn relay_bidirectional_observed(
    inbound: &mut TcpStream,
    upstream: &mut UpstreamStream,
    logger: &RuntimeLogger,
    peer_addr: std::net::SocketAddr,
    target: &str,
) -> Result<(u64, u64)> {
    let mut inbound_observed = ObservedStream::new(
        inbound,
        logger.clone(),
        peer_addr,
        target.to_owned(),
        RelayReadSide::Inbound,
    );
    let mut upstream_observed = ObservedStream::new(
        upstream,
        logger.clone(),
        peer_addr,
        target.to_owned(),
        RelayReadSide::Upstream,
    );
    let copied = copy_bidirectional(&mut inbound_observed, &mut upstream_observed).await?;
    Ok(copied)
}

pub fn format_target(request: &ConnectRequest) -> String {
    match &request.target {
        TargetAddr::Ip(ip) => format!("{ip}:{}", request.port),
        TargetAddr::Domain(domain) => format!("{domain}:{}", request.port),
    }
}

#[derive(Clone, Copy)]
enum RelayReadSide {
    Inbound,
    Upstream,
}

impl RelayReadSide {
    fn message(self) -> &'static str {
        match self {
            RelayReadSide::Inbound => "ingress relay 首次收到来自本地 helper 的字节",
            RelayReadSide::Upstream => "ingress relay 首次收到来自上游的字节",
        }
    }

    fn tracing_side(self) -> &'static str {
        match self {
            RelayReadSide::Inbound => "from_helper",
            RelayReadSide::Upstream => "from_upstream",
        }
    }
}

struct ObservedStream<'a, S> {
    inner: &'a mut S,
    logger: RuntimeLogger,
    peer_addr: std::net::SocketAddr,
    target: String,
    side: RelayReadSide,
    first_read_logged: bool,
}

impl<'a, S> ObservedStream<'a, S> {
    fn new(
        inner: &'a mut S,
        logger: RuntimeLogger,
        peer_addr: std::net::SocketAddr,
        target: String,
        side: RelayReadSide,
    ) -> Self {
        Self {
            inner,
            logger,
            peer_addr,
            target,
            side,
            first_read_logged: false,
        }
    }
}

impl<S> AsyncRead for ObservedStream<'_, S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let filled_before = buf.filled().len();
        match Pin::new(&mut *self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let first_read_bytes = buf.filled().len().saturating_sub(filled_before);
                if !self.first_read_logged && first_read_bytes > 0 {
                    self.first_read_logged = true;
                    self.logger.log(format!(
                        "{}，来源 {} -> 目标 {}，bytes={first_read_bytes}",
                        self.side.message(),
                        self.peer_addr,
                        self.target
                    ));
                    info!(
                        peer = %self.peer_addr,
                        target = %self.target,
                        relay_side = self.side.tracing_side(),
                        first_read_bytes,
                        "ingress relay observed first bytes"
                    );
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl<S> AsyncWrite for ObservedStream<'_, S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.inner).poll_shutdown(cx)
    }
}
