use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use px_proto::{ClientConfig, ConnectRequest, ConnectResponse, StatusCode};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::session;
use crate::socket::apply_socket_options;
use crate::upstream::UpstreamConnector;
use crate::RuntimeLogger;

pub async fn run_listener(
    listener: TcpListener,
    config: ClientConfig,
    upstream: Arc<UpstreamConnector>,
    logger: RuntimeLogger,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let listen_addr = listener
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    logger.log(format!(
        "客户端开始监听，本地 ingress: {}，服务端: {}",
        listen_addr, config.server_addr
    ));

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        let config = config.clone();
                        let upstream = upstream.clone();
                        let logger = logger.clone();
                        tokio::spawn(async move {
                            if let Err(error) =
                                handle_client(stream, peer_addr, config, upstream, logger.clone())
                                    .await
                            {
                                let error_text = format_error_chain(&error);
                                let message =
                                    format!("ingress 会话失败，来源 {peer_addr}: {error_text}");
                                logger.log(message.clone());
                                error!(
                                    peer = %peer_addr,
                                    error = %error_text,
                                    "ingress session failed"
                                );
                            }
                        });
                    }
                    Err(error) => {
                        let message = format!("接受本地 ingress 连接失败: {error}");
                        logger.log(message.clone());
                        error!(error = %error, "ingress accept failed");
                        break;
                    }
                }
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    logger.log("本地 ingress 已停止。");
                    break;
                }
            }
        }
    }
}

pub async fn handle_client(
    mut inbound: TcpStream,
    peer_addr: std::net::SocketAddr,
    config: ClientConfig,
    upstream: Arc<UpstreamConnector>,
    logger: RuntimeLogger,
) -> Result<()> {
    apply_socket_options(&inbound)?;

    let request = ConnectRequest::read_from(&mut inbound)
        .await
        .context("ingress request parse failed")?;
    let target = session::format_target(&request);
    let upstream_open_started_at = Instant::now();
    logger.log(format!(
        "ingress 会话已解析请求，来源 {peer_addr} -> 目标 {target}"
    ));
    info!(peer = %peer_addr, target = %target, "ingress session parsed request");

    let mut upstream = open_upstream_or_reply(
        &mut inbound,
        peer_addr,
        &request,
        &config,
        &upstream,
        &logger,
    )
    .await?;
    let upstream_ready_ms = upstream_open_started_at.elapsed().as_millis();
    logger.log(format!(
        "ingress 上游已就绪，来源 {peer_addr} -> 目标 {target}，upstream_ready_ms={upstream_ready_ms}"
    ));
    info!(
        peer = %peer_addr,
        target = %target,
        upstream_ready_ms,
        "ingress upstream stream is ready"
    );
    send_response(&mut inbound, StatusCode::Ok).await?;
    logger.log(format!(
        "ingress 已向本地 helper 返回 Ok，来源 {peer_addr} -> 目标 {target}"
    ));
    info!(
        peer = %peer_addr,
        target = %target,
        "ingress sent ok response to local helper"
    );

    let relay_started_at = Instant::now();
    let (inbound_to_upstream_bytes, upstream_to_inbound_bytes) =
        session::relay_bidirectional_observed(
            &mut inbound,
            &mut upstream,
            &logger,
            peer_addr,
            &target,
        )
        .await?;
    let relay_ms = relay_started_at.elapsed().as_millis();
    logger.log(format!(
        "ingress relay finished，来源 {peer_addr} -> 目标 {target}，relay_ms={relay_ms}，inbound_to_upstream_bytes={inbound_to_upstream_bytes}，upstream_to_inbound_bytes={upstream_to_inbound_bytes}"
    ));
    info!(
        peer = %peer_addr,
        target = %target,
        relay_ms,
        inbound_to_upstream_bytes,
        upstream_to_inbound_bytes,
        "ingress relay finished"
    );
    Ok(())
}

async fn open_upstream_or_reply(
    inbound: &mut TcpStream,
    peer_addr: std::net::SocketAddr,
    request: &ConnectRequest,
    config: &ClientConfig,
    upstream: &Arc<UpstreamConnector>,
    logger: &RuntimeLogger,
) -> Result<session::UpstreamStream> {
    match session::open_upstream_stream(request, config, upstream).await {
        Ok(stream) => Ok(stream),
        Err(session::OpenStreamError::Connect(error)) => {
            logger.log(format!(
                "ingress 上游连接失败，来源 {peer_addr} -> 目标 {}: {error}",
                session::format_target(request)
            ));
            warn!(
                peer = %peer_addr,
                target = %session::format_target(request),
                error = %error,
                "ingress upstream connect failed"
            );
            let _ = send_response(inbound, StatusCode::InternalError).await;
            Err(error).context("ingress upstream connect failed")
        }
        Err(session::OpenStreamError::Timeout(error)) => {
            logger.log(format!(
                "ingress 上游连接超时，来源 {peer_addr} -> 目标 {}，timeout_ms={}: {error}",
                session::format_target(request),
                config.connect_timeout_ms
            ));
            warn!(
                peer = %peer_addr,
                target = %session::format_target(request),
                timeout_ms = config.connect_timeout_ms,
                error = %error,
                "ingress upstream connect timeout"
            );
            let _ = send_response(inbound, StatusCode::Timeout).await;
            Err(error).context("ingress upstream timeout")
        }
        Err(session::OpenStreamError::ResponseRead(error)) => {
            logger.log(format!(
                "ingress 读取上游建连响应失败，来源 {peer_addr} -> 目标 {}: {error}",
                session::format_target(request)
            ));
            warn!(
                peer = %peer_addr,
                target = %session::format_target(request),
                error = %error,
                "failed to read ingress upstream connect response"
            );
            let _ = send_response(inbound, StatusCode::InternalError).await;
            Err(error).context("failed to read ingress upstream connect response")
        }
        Err(session::OpenStreamError::Refused(status)) => {
            logger.log(format!(
                "ingress 上游拒绝请求，来源 {peer_addr} -> 目标 {}，status={status:?}",
                session::format_target(request)
            ));
            warn!(
                peer = %peer_addr,
                target = %session::format_target(request),
                status = ?status,
                "ingress upstream refused request"
            );
            let _ = send_response(inbound, status).await;
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!(
                    "ingress upstream refused target {} with status {:?}",
                    session::format_target(request),
                    status
                ),
            )
            .into())
        }
    }
}

async fn send_response(stream: &mut TcpStream, status: StatusCode) -> Result<()> {
    ConnectResponse { status, reason: 0 }
        .write_to(stream)
        .await?;
    Ok(())
}

fn format_error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(": ")
}
