use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use px_proto::{ClientConfig, ConnectRequest, StatusCode, TargetAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tracing::{debug, error, warn};

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
                        let upstream = upstream.clone();
                        let logger = logger.clone();
                        tokio::spawn(async move {
                            if let Err(error) = handle_client(stream, peer_addr, config, upstream).await {
                                let error_text = format_error_chain(&error);
                                if should_surface_client_error(&error) {
                                    let message =
                                        format!("客户端会话失败，来源 {peer_addr}: {error_text}");
                                    logger.log(message.clone());
                                    error!(
                                        peer = %peer_addr,
                                        error = %error_text,
                                        "client session failed"
                                    );
                                } else {
                                    debug!(
                                        peer = %peer_addr,
                                        error = %error_text,
                                        "ignored unsupported socks5 command"
                                    );
                                }
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

pub async fn handle_client(
    mut inbound: TcpStream,
    _peer_addr: SocketAddr,
    config: ClientConfig,
    upstream: Arc<UpstreamConnector>,
) -> Result<()> {
    apply_socket_options(&inbound)?;

    let request = accept_socks5_connect(&mut inbound)
        .await
        .context("socks5 accept failed")?;

    let mut upstream = open_upstream_or_reply(&mut inbound, &request, &config, &upstream).await?;

    send_success_reply(&mut inbound).await?;

    let _ = session::relay_bidirectional(&mut inbound, &mut upstream).await?;
    Ok(())
}

async fn open_upstream_or_reply(
    inbound: &mut TcpStream,
    request: &ConnectRequest,
    config: &ClientConfig,
    upstream: &Arc<UpstreamConnector>,
) -> Result<session::UpstreamStream> {
    match session::open_upstream_stream(request, config, upstream).await {
        Ok(stream) => Ok(stream),
        Err(session::OpenStreamError::Connect(error)) => {
            warn!(
                target = %session::format_target(request),
                error = %error,
                "upstream connect failed"
            );
            let _ = send_failure_reply(inbound, 0x01).await;
            Err(error).context("upstream connect failed")
        }
        Err(session::OpenStreamError::Timeout(error)) => {
            warn!(
                target = %session::format_target(request),
                timeout_ms = config.connect_timeout_ms,
                error = %error,
                "upstream connect timeout"
            );
            let _ = send_failure_reply(inbound, 0x04).await;
            Err(error).context("upstream timeout")
        }
        Err(session::OpenStreamError::ResponseRead(error)) => {
            warn!(
                target = %session::format_target(request),
                error = %error,
                "failed to read upstream connect response"
            );
            let _ = send_failure_reply(inbound, 0x01).await;
            Err(error).context("failed to read upstream connect response")
        }
        Err(session::OpenStreamError::Refused(status)) => {
            warn!(
                target = %session::format_target(request),
                status = ?status,
                "upstream refused request"
            );
            let _ = send_failure_reply(inbound, map_upstream_status_to_socks(status)).await;
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!(
                    "upstream refused target {} with status {:?}",
                    session::format_target(request),
                    status
                ),
            )
            .into())
        }
    }
}

pub(crate) async fn accept_socks5_connect(stream: &mut TcpStream) -> Result<ConnectRequest> {
    handshake(stream).await.context("socks5 handshake failed")?;
    read_request(stream)
        .await
        .context("socks5 request parse failed")
}

async fn handshake(stream: &mut TcpStream) -> Result<()> {
    let version = stream
        .read_u8()
        .await
        .context("failed to read socks5 handshake version")?;
    if version != 0x05 {
        return Err(anyhow!("unsupported socks version: {version}"));
    }
    let methods_len = stream
        .read_u8()
        .await
        .context("failed to read socks5 auth methods length")? as usize;
    let mut methods = vec![0_u8; methods_len];
    stream
        .read_exact(&mut methods)
        .await
        .context("failed to read socks5 auth methods")?;
    if !methods.contains(&0x00) {
        stream.write_all(&[0x05, 0xff]).await?;
        return Err(anyhow!("client does not support no-auth socks5"));
    }
    stream.write_all(&[0x05, 0x00]).await?;
    Ok(())
}

async fn read_request(stream: &mut TcpStream) -> Result<ConnectRequest> {
    let version = stream
        .read_u8()
        .await
        .context("failed to read socks5 request version")?;
    if version != 0x05 {
        return Err(anyhow!("invalid request socks version: {version}"));
    }
    let cmd = stream
        .read_u8()
        .await
        .context("failed to read socks5 request command")?;
    if cmd != 0x01 {
        stream
            .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        return Err(anyhow!("only CONNECT is supported"));
    }
    let _reserved = stream
        .read_u8()
        .await
        .context("failed to read socks5 reserved byte")?;
    let addr_type = stream
        .read_u8()
        .await
        .context("failed to read socks5 address type")?;
    let target = match addr_type {
        0x01 => {
            let mut buf = [0_u8; 4];
            stream
                .read_exact(&mut buf)
                .await
                .context("failed to read ipv4 target address")?;
            TargetAddr::Ip(IpAddr::from(buf))
        }
        0x03 => {
            let len = stream
                .read_u8()
                .await
                .context("failed to read domain length")? as usize;
            let mut buf = vec![0_u8; len];
            stream
                .read_exact(&mut buf)
                .await
                .context("failed to read domain target address")?;
            let domain = String::from_utf8(buf)?;
            TargetAddr::Domain(domain)
        }
        0x04 => {
            let mut buf = [0_u8; 16];
            stream
                .read_exact(&mut buf)
                .await
                .context("failed to read ipv6 target address")?;
            TargetAddr::Ip(IpAddr::from(buf))
        }
        _ => {
            stream
                .write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await?;
            return Err(anyhow!("unsupported address type: {addr_type}"));
        }
    };
    let port = stream
        .read_u16()
        .await
        .context("failed to read target port")?;
    Ok(ConnectRequest { target, port })
}

pub(crate) async fn send_success_reply(stream: &mut TcpStream) -> Result<()> {
    stream
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    Ok(())
}

pub(crate) async fn send_failure_reply(stream: &mut TcpStream, reply_code: u8) -> Result<()> {
    stream
        .write_all(&[0x05, reply_code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    Ok(())
}

pub(crate) fn map_upstream_status_to_socks(status: StatusCode) -> u8 {
    match status {
        StatusCode::Ok => 0x00,
        StatusCode::BadRequest | StatusCode::TlsAuthFailed | StatusCode::InternalError => 0x01,
        StatusCode::TargetConnectFailed => 0x05,
        StatusCode::Timeout => 0x04,
    }
}

pub(crate) fn format_error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(": ")
}

pub(crate) fn should_surface_client_error(error: &anyhow::Error) -> bool {
    !error
        .chain()
        .any(|cause| cause.to_string().contains("only CONNECT is supported"))
}
