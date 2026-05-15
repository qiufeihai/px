use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use px_proto::{ClientConfig, ConnectResponse, StatusCode, TargetAddr};
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::upstream;

pub async fn handle_client(
    mut inbound: TcpStream,
    _peer_addr: SocketAddr,
    config: ClientConfig,
) -> Result<()> {
    handshake(&mut inbound).await?;
    let request = read_request(&mut inbound).await?;

    let timeout_ms = Duration::from_millis(config.connect_timeout_ms);
    let mut upstream = tokio::time::timeout(timeout_ms, upstream::connect(&config, &request))
        .await
        .context("upstream timeout")??;

    ConnectResponse::read_from(&mut upstream)
        .await
        .and_then(|response| match response.status {
            StatusCode::Ok => Ok(response),
            status => Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("upstream refused with status {:?}", status),
            )),
        })?;

    inbound
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    inbound.flush().await?;

    relay(&mut inbound, &mut upstream).await?;
    Ok(())
}

async fn handshake(stream: &mut TcpStream) -> Result<()> {
    let version = stream.read_u8().await?;
    if version != 0x05 {
        return Err(anyhow!("unsupported socks version: {version}"));
    }
    let methods_len = stream.read_u8().await? as usize;
    let mut methods = vec![0_u8; methods_len];
    stream.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        stream.write_all(&[0x05, 0xff]).await?;
        return Err(anyhow!("client does not support no-auth socks5"));
    }
    stream.write_all(&[0x05, 0x00]).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_request(stream: &mut TcpStream) -> Result<px_proto::ConnectRequest> {
    let version = stream.read_u8().await?;
    if version != 0x05 {
        return Err(anyhow!("invalid request socks version: {version}"));
    }
    let cmd = stream.read_u8().await?;
    if cmd != 0x01 {
        stream
            .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        return Err(anyhow!("only CONNECT is supported"));
    }
    let _reserved = stream.read_u8().await?;
    let addr_type = stream.read_u8().await?;
    let target = match addr_type {
        0x01 => {
            let mut buf = [0_u8; 4];
            stream.read_exact(&mut buf).await?;
            TargetAddr::Ip(IpAddr::from(buf))
        }
        0x03 => {
            let len = stream.read_u8().await? as usize;
            let mut buf = vec![0_u8; len];
            stream.read_exact(&mut buf).await?;
            let domain = String::from_utf8(buf)?;
            TargetAddr::Domain(domain)
        }
        0x04 => {
            let mut buf = [0_u8; 16];
            stream.read_exact(&mut buf).await?;
            TargetAddr::Ip(IpAddr::from(buf))
        }
        _ => return Err(anyhow!("unsupported address type: {addr_type}")),
    };
    let port = stream.read_u16().await?;
    Ok(px_proto::ConnectRequest { target, port })
}

async fn relay(inbound: &mut TcpStream, upstream: &mut TlsStream<TcpStream>) -> Result<()> {
    copy_bidirectional(inbound, upstream).await?;
    Ok(())
}
