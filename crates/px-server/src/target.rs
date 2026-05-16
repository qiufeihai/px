use anyhow::{Context, Result};
use px_proto::{ConnectRequest, TargetAddr};
use tokio::net::TcpStream;

pub async fn connect_target(request: &ConnectRequest) -> Result<TcpStream> {
    let stream = match &request.target {
        TargetAddr::Ip(ip) => TcpStream::connect((*ip, request.port)).await,
        TargetAddr::Domain(domain) => TcpStream::connect((domain.as_str(), request.port)).await,
    };
    let stream = stream.with_context(|| format!("failed to connect target port {}", request.port))?;

    Ok(stream)
}
