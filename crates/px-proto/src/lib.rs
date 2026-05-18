use std::io;
use std::net::IpAddr;
use std::path::Path;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAGIC: &[u8; 4] = b"PXT1";
pub const VERSION: u8 = 1;
pub const CMD_CONNECT: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub listen_addr: String,
    pub tls_cert_path: String,
    pub tls_key_path: String,
    pub connect_timeout_ms: u64,
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_addr: String,
    pub server_cert_path: String,
    pub local_socks_addr: String,
    pub connect_timeout_ms: u64,
    pub log_level: String,
    #[serde(default)]
    pub tun: TunConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TunConfig {
    pub enabled: bool,
    pub helper_path: String,
    pub device_name: String,
    pub primary_interface: String,
    pub ipv4_addr: String,
    pub mtu: u16,
}

impl Default for TunConfig {
    fn default() -> Self {
        let (helper_path, device_name, ipv4_addr, mtu) = if cfg!(target_os = "windows") {
            (
                "bin/px-tun-helper.exe".to_string(),
                "wintun".to_string(),
                "192.168.123.1".to_string(),
                1500,
            )
        } else if cfg!(target_os = "macos") {
            (
                "bin/px-tun-helper".to_string(),
                "utun233".to_string(),
                "198.18.0.1".to_string(),
                1500,
            )
        } else {
            (
                "bin/tun2socks".to_string(),
                "utun233".to_string(),
                "198.18.0.1".to_string(),
                1500,
            )
        };

        Self {
            enabled: false,
            helper_path,
            device_name,
            primary_interface: String::new(),
            ipv4_addr,
            mtu,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetAddr {
    Ip(IpAddr),
    Domain(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectRequest {
    pub target: TargetAddr,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StatusCode {
    Ok = 0,
    BadRequest = 1,
    TlsAuthFailed = 2,
    TargetConnectFailed = 3,
    Timeout = 4,
    InternalError = 5,
}

impl StatusCode {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Ok),
            1 => Some(Self::BadRequest),
            2 => Some(Self::TlsAuthFailed),
            3 => Some(Self::TargetConnectFailed),
            4 => Some(Self::Timeout),
            5 => Some(Self::InternalError),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectResponse {
    pub status: StatusCode,
    pub reason: u16,
}

impl ConnectRequest {
    pub async fn read_from<R>(reader: &mut R) -> io::Result<Self>
    where
        R: AsyncRead + Unpin,
    {
        let mut magic = [0_u8; 4];
        reader.read_exact(&mut magic).await?;
        if &magic != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid magic"));
        }

        let version = reader.read_u8().await?;
        if version != VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid version"));
        }

        let cmd = reader.read_u8().await?;
        if cmd != CMD_CONNECT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported cmd"));
        }

        let addr_type = reader.read_u8().await?;
        let _reserved = reader.read_u8().await?;
        let port = reader.read_u16().await?;
        let addr_len = reader.read_u16().await?;

        let target = match addr_type {
            1 => {
                let mut ip = [0_u8; 4];
                reader.read_exact(&mut ip).await?;
                TargetAddr::Ip(IpAddr::from(ip))
            }
            2 => {
                let mut ip = [0_u8; 16];
                reader.read_exact(&mut ip).await?;
                TargetAddr::Ip(IpAddr::from(ip))
            }
            3 => {
                let mut buf = vec![0_u8; usize::from(addr_len)];
                reader.read_exact(&mut buf).await?;
                let domain = String::from_utf8(buf).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "domain is not valid utf-8")
                })?;
                TargetAddr::Domain(domain)
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported address type",
                ))
            }
        };

        Ok(Self { target, port })
    }

    pub async fn write_to<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        writer.write_all(MAGIC).await?;
        writer.write_u8(VERSION).await?;
        writer.write_u8(CMD_CONNECT).await?;

        match &self.target {
            TargetAddr::Ip(IpAddr::V4(ip)) => {
                writer.write_u8(1).await?;
                writer.write_u8(0).await?;
                writer.write_u16(self.port).await?;
                writer.write_u16(0).await?;
                writer.write_all(&ip.octets()).await?;
            }
            TargetAddr::Ip(IpAddr::V6(ip)) => {
                writer.write_u8(2).await?;
                writer.write_u8(0).await?;
                writer.write_u16(self.port).await?;
                writer.write_u16(0).await?;
                writer.write_all(&ip.octets()).await?;
            }
            TargetAddr::Domain(domain) => {
                let bytes = domain.as_bytes();
                writer.write_u8(3).await?;
                writer.write_u8(0).await?;
                writer.write_u16(self.port).await?;
                writer.write_u16(bytes.len() as u16).await?;
                writer.write_all(bytes).await?;
            }
        }

        writer.flush().await
    }
}

impl ConnectResponse {
    pub async fn read_from<R>(reader: &mut R) -> io::Result<Self>
    where
        R: AsyncRead + Unpin,
    {
        let status = reader.read_u8().await?;
        let _reserved = reader.read_u8().await?;
        let reason = reader.read_u16().await?;
        let status = StatusCode::from_u8(status)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid status code"))?;
        Ok(Self { status, reason })
    }

    pub async fn write_to<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        writer.write_u8(self.status as u8).await?;
        writer.write_u8(0).await?;
        writer.write_u16(self.reason).await?;
        writer.flush().await
    }
}

pub fn load_client_config(path: &Path) -> Result<ClientConfig> {
    let raw = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&raw)?)
}

pub fn load_server_config(path: &Path) -> Result<ServerConfig> {
    let raw = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&raw)?)
}

pub fn save_client_config(path: &Path, config: &ClientConfig) -> Result<()> {
    let raw = toml::to_string_pretty(config)?;
    std::fs::write(path, raw)?;
    Ok(())
}

pub fn parse_target(value: &str) -> Result<ConnectRequest> {
    let (host, port) = value
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("target must be host:port"))?;
    let port = port.parse::<u16>()?;
    let target = if let Ok(ip) = host.parse::<IpAddr>() {
        TargetAddr::Ip(ip)
    } else {
        if host.is_empty() {
            bail!("target host is empty");
        }
        TargetAddr::Domain(host.to_string())
    };
    Ok(ConnectRequest { target, port })
}
