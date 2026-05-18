use std::time::Duration;

use anyhow::Result;
use socket2::{SockRef, TcpKeepalive};
use tokio::net::TcpStream;

pub fn apply_socket_options(stream: &TcpStream) -> Result<()> {
    stream.set_nodelay(true)?;
    let sock = SockRef::from(stream);
    let keepalive = TcpKeepalive::new().with_time(Duration::from_secs(30));
    sock.set_tcp_keepalive(&keepalive)?;
    Ok(())
}
