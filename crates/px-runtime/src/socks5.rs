use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use px_proto::{ClientConfig, ConnectResponse, StatusCode, TargetAddr};
use socket2::{SockRef, TcpKeepalive};
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::upstream::UpstreamConnector;

pub async fn handle_client(
    mut inbound: TcpStream,
    _peer_addr: SocketAddr,
    config: ClientConfig,
    upstream: Arc<UpstreamConnector>,
) -> Result<()> {
    apply_socket_options(&inbound)?;
    handshake(&mut inbound)
        .await
        .context("socks5 handshake failed")?;
    let request = read_request(&mut inbound)
        .await
        .context("socks5 request parse failed")?;

    let timeout_ms = Duration::from_millis(config.connect_timeout_ms);
    let mut upstream = tokio::time::timeout(timeout_ms, upstream.connect(&request))
        .await
        .context("upstream timeout")?
        .context("upstream connect failed")?;

    let response = ConnectResponse::read_from(&mut upstream)
        .await
        .context("failed to read upstream connect response")?;
    match response.status {
        StatusCode::Ok => {}
        status => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("upstream refused with status {:?}", status),
            )
            .into());
        }
    }

    inbound
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;

    relay(&mut inbound, &mut upstream).await?;
    Ok(())
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

async fn read_request(stream: &mut TcpStream) -> Result<px_proto::ConnectRequest> {
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
        _ => return Err(anyhow!("unsupported address type: {addr_type}")),
    };
    let port = stream
        .read_u16()
        .await
        .context("failed to read target port")?;
    Ok(px_proto::ConnectRequest { target, port })
}

async fn relay(inbound: &mut TcpStream, upstream: &mut TlsStream<TcpStream>) -> Result<()> {
    copy_bidirectional(inbound, upstream).await?;
    Ok(())
}

fn apply_socket_options(stream: &TcpStream) -> Result<()> {
    stream.set_nodelay(true)?;
    let sock = SockRef::from(stream);
    let keepalive = TcpKeepalive::new().with_time(Duration::from_secs(30));
    sock.set_tcp_keepalive(&keepalive)?;
    Ok(())
}
