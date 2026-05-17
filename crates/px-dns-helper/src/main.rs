use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

#[derive(Clone)]
struct Config {
    listen_addr: SocketAddr,
    socks_addr: SocketAddr,
    upstreams: Vec<SocketAddr>,
    timeout: Duration,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_args()?;
    run(config).await
}

impl Config {
    fn from_args() -> Result<Self> {
        let mut listen_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 53));
        let mut socks_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 7777));
        let mut upstreams = Vec::new();
        let mut timeout = Duration::from_secs(5);

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--listen" => {
                    let value = args.next().context("missing value for --listen")?;
                    listen_addr = value.parse().context("invalid --listen address")?;
                }
                "--socks" => {
                    let value = args.next().context("missing value for --socks")?;
                    socks_addr = value.parse().context("invalid --socks address")?;
                }
                "--upstream" => {
                    let value = args.next().context("missing value for --upstream")?;
                    upstreams.push(value.parse().context("invalid --upstream address")?);
                }
                "--timeout-ms" => {
                    let value = args.next().context("missing value for --timeout-ms")?;
                    timeout = Duration::from_millis(value.parse().context("invalid --timeout-ms")?);
                }
                other => bail!("unknown argument: {other}"),
            }
        }

        if upstreams.is_empty() {
            upstreams.push(SocketAddr::from((Ipv4Addr::new(1, 1, 1, 1), 53)));
            upstreams.push(SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 53)));
        }

        Ok(Self {
            listen_addr,
            socks_addr,
            upstreams,
            timeout,
        })
    }
}

async fn run(config: Config) -> Result<()> {
    let socket = Arc::new(
        UdpSocket::bind(config.listen_addr)
            .await
            .with_context(|| format!("failed to bind dns helper {}", config.listen_addr))?,
    );
    let mut buffer = vec![0_u8; 4096];

    loop {
        let (len, client_addr) = socket.recv_from(&mut buffer).await?;
        let query = buffer[..len].to_vec();
        let socket = socket.clone();
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_query(socket, client_addr, query, config).await {
                eprintln!("dns query from {client_addr} failed: {error}");
            }
        });
    }
}

async fn handle_query(
    socket: Arc<UdpSocket>,
    client_addr: SocketAddr,
    query: Vec<u8>,
    config: Config,
) -> Result<()> {
    let response = resolve_with_fallback(&query, &config).await?;
    socket
        .send_to(&response, client_addr)
        .await
        .with_context(|| format!("failed to send dns response to {client_addr}"))?;
    Ok(())
}

async fn resolve_with_fallback(query: &[u8], config: &Config) -> Result<Vec<u8>> {
    let mut last_error = None;
    for upstream in &config.upstreams {
        match timeout(config.timeout, resolve_once(query, config.socks_addr, *upstream)).await {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(error)) => last_error = Some(error),
            Err(error) => last_error = Some(error.into()),
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no dns upstream configured")))
}

async fn resolve_once(
    query: &[u8],
    socks_addr: SocketAddr,
    upstream_addr: SocketAddr,
) -> Result<Vec<u8>> {
    let mut stream = TcpStream::connect(socks_addr)
        .await
        .with_context(|| format!("failed to connect socks {socks_addr}"))?;
    socks5_connect(&mut stream, upstream_addr).await?;

    let length = u16::try_from(query.len()).context("dns query too large")?;
    stream.write_u16(length).await?;
    stream.write_all(query).await?;
    stream.flush().await?;

    let response_len = stream.read_u16().await? as usize;
    let mut response = vec![0_u8; response_len];
    stream.read_exact(&mut response).await?;
    Ok(response)
}

async fn socks5_connect(stream: &mut TcpStream, target: SocketAddr) -> Result<()> {
    stream.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut auth_response = [0_u8; 2];
    stream.read_exact(&mut auth_response).await?;
    if auth_response != [0x05, 0x00] {
        bail!("socks5 no-auth handshake failed");
    }

    match target.ip() {
        IpAddr::V4(ip) => {
            stream.write_all(&[0x05, 0x01, 0x00, 0x01]).await?;
            stream.write_all(&ip.octets()).await?;
        }
        IpAddr::V6(ip) => {
            stream.write_all(&[0x05, 0x01, 0x00, 0x04]).await?;
            stream.write_all(&ip.octets()).await?;
        }
    }
    stream.write_u16(target.port()).await?;
    stream.flush().await?;

    let mut head = [0_u8; 4];
    stream.read_exact(&mut head).await?;
    if head[1] != 0x00 {
        bail!("socks5 connect failed with code {}", head[1]);
    }

    match head[3] {
        0x01 => {
            let mut rest = [0_u8; 6];
            stream.read_exact(&mut rest).await?;
        }
        0x04 => {
            let mut rest = [0_u8; 18];
            stream.read_exact(&mut rest).await?;
        }
        0x03 => {
            let len = stream.read_u8().await? as usize;
            let mut rest = vec![0_u8; len + 2];
            stream.read_exact(&mut rest).await?;
        }
        atyp => bail!("unexpected socks5 atyp {atyp}"),
    }

    Ok(())
}
