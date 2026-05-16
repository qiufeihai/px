use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use px_proto::{ConnectRequest, TargetAddr};
use tokio::net::lookup_host;
use tokio::net::TcpStream;
use tracing::debug;

const DNS_CACHE_TTL: Duration = Duration::from_secs(60);
const DNS_CACHE_CAPACITY: usize = 128;

pub async fn connect_target(request: &ConnectRequest) -> Result<TcpStream> {
    let stream = match &request.target {
        TargetAddr::Ip(ip) => TcpStream::connect((*ip, request.port))
            .await
            .with_context(|| format!("failed to connect target port {}", request.port))?,
        TargetAddr::Domain(domain) => connect_domain_target(domain, request.port)
            .await
            .with_context(|| format!("failed to connect target port {}", request.port))?,
    };

    Ok(stream)
}

async fn connect_domain_target(domain: &str, port: u16) -> Result<TcpStream> {
    let ips = get_cached_ips(domain).unwrap_or_else(|| Vec::new());
    if !ips.is_empty() {
        debug!(domain, ip_count = ips.len(), "dns cache hit");
        if let Ok(stream) = connect_resolved_ips(&ips, port).await {
            return Ok(stream);
        }
    } else {
        debug!(domain, "dns cache miss");
    }

    let resolved_ips = resolve_domain(domain, port).await?;
    store_cached_ips(domain, resolved_ips.clone());
    connect_resolved_ips(&resolved_ips, port).await
}

async fn resolve_domain(domain: &str, port: u16) -> Result<Vec<IpAddr>> {
    let resolved = lookup_host((domain, port))
        .await
        .with_context(|| format!("failed to resolve target domain {domain}"))?;
    let mut ips = Vec::new();
    for addr in resolved {
        let ip = addr.ip();
        if !ips.contains(&ip) {
            ips.push(ip);
        }
    }
    if ips.is_empty() {
        anyhow::bail!("target domain {domain} resolved to no addresses");
    }
    Ok(ips)
}

async fn connect_resolved_ips(ips: &[IpAddr], port: u16) -> Result<TcpStream> {
    let mut last_error = None;
    for ip in ips {
        match TcpStream::connect(SocketAddr::new(*ip, port)).await {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("no resolved target addresses"))
        .into())
}

fn get_cached_ips(domain: &str) -> Option<Vec<IpAddr>> {
    let cache = dns_cache();
    let mut guard = cache.lock().expect("dns cache poisoned");
    let (expires_at, ips) = guard
        .entries
        .get(domain)
        .map(|entry| (entry.expires_at, entry.ips.clone()))?;
    if expires_at <= Instant::now() {
        guard.entries.remove(domain);
        return None;
    }
    Some(ips)
}

fn store_cached_ips(domain: &str, ips: Vec<IpAddr>) {
    let cache = dns_cache();
    let mut guard = cache.lock().expect("dns cache poisoned");
    guard
        .entries
        .retain(|_, entry| entry.expires_at > Instant::now());
    if guard.entries.len() >= DNS_CACHE_CAPACITY {
        if let Some(evict_key) = guard.entries.keys().next().cloned() {
            guard.entries.remove(&evict_key);
        }
    }
    guard.entries.insert(
        domain.to_string(),
        DnsCacheEntry {
            ips,
            expires_at: Instant::now() + DNS_CACHE_TTL,
        },
    );
}

fn dns_cache() -> &'static Mutex<DnsCache> {
    static CACHE: OnceLock<Mutex<DnsCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(DnsCache::default()))
}

#[derive(Default)]
struct DnsCache {
    entries: HashMap<String, DnsCacheEntry>,
}

struct DnsCacheEntry {
    ips: Vec<IpAddr>,
    expires_at: Instant,
}
