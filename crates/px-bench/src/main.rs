use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let socks_addr = get_flag(&args, "--socks").unwrap_or_else(|| "127.0.0.1:7777".to_string());
    let target = get_flag(&args, "--target").unwrap_or_else(|| "example.com:80".to_string());
    let iterations = get_flag(&args, "--iterations")
        .unwrap_or_else(|| "5".to_string())
        .parse::<usize>()?;

    let direct = benchmark_direct(&target, iterations).await?;
    let proxied = benchmark_socks(&socks_addr, &target, iterations).await?;

    println!("direct_avg_ms={:.2}", avg_ms(&direct));
    println!("direct_p95_ms={:.2}", percentile_ms(&direct, 95));
    println!("direct_p99_ms={:.2}", percentile_ms(&direct, 99));
    println!("socks_avg_ms={:.2}", avg_ms(&proxied));
    println!("socks_p95_ms={:.2}", percentile_ms(&proxied, 95));
    println!("socks_p99_ms={:.2}", percentile_ms(&proxied, 99));
    println!("delta_ms={:.2}", avg_ms(&proxied) - avg_ms(&direct));
    Ok(())
}

fn get_flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

async fn benchmark_direct(target: &str, iterations: usize) -> Result<Vec<Duration>> {
    let mut samples = Vec::with_capacity(iterations);
    let request = px_proto::parse_target(target)?;
    for _ in 0..iterations {
        let started = Instant::now();
        let stream = match request.target.clone() {
            px_proto::TargetAddr::Ip(ip) => TcpStream::connect((ip, request.port)).await?,
            px_proto::TargetAddr::Domain(domain) => {
                TcpStream::connect((domain.as_str(), request.port)).await?
            }
        };
        drop(stream);
        samples.push(started.elapsed());
    }
    Ok(samples)
}

async fn benchmark_socks(socks_addr: &str, target: &str, iterations: usize) -> Result<Vec<Duration>> {
    let mut samples = Vec::with_capacity(iterations);
    let request = px_proto::parse_target(target)?;
    for _ in 0..iterations {
        let started = Instant::now();
        let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(socks_addr)).await??;
        stream.write_all(&[0x05, 0x01, 0x00]).await?;
        let mut auth = [0_u8; 2];
        stream.read_exact(&mut auth).await?;
        if auth != [0x05, 0x00] {
            return Err(anyhow!("unexpected socks auth response"));
        }

        match request.target.clone() {
            px_proto::TargetAddr::Ip(std::net::IpAddr::V4(ip)) => {
                stream.write_all(&[0x05, 0x01, 0x00, 0x01]).await?;
                stream.write_all(&ip.octets()).await?;
            }
            px_proto::TargetAddr::Ip(std::net::IpAddr::V6(ip)) => {
                stream.write_all(&[0x05, 0x01, 0x00, 0x04]).await?;
                stream.write_all(&ip.octets()).await?;
            }
            px_proto::TargetAddr::Domain(domain) => {
                stream.write_all(&[0x05, 0x01, 0x00, 0x03, domain.len() as u8]).await?;
                stream.write_all(domain.as_bytes()).await?;
            }
        }

        stream.write_u16(request.port).await?;
        let mut response = [0_u8; 10];
        stream.read_exact(&mut response).await?;
        if response[1] != 0x00 {
            return Err(anyhow!("proxy connect failed with code {}", response[1]));
        }

        drop(stream);
        samples.push(started.elapsed());
    }
    Ok(samples)
}

fn avg_ms(samples: &[Duration]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let total: f64 = samples.iter().map(|s| s.as_secs_f64() * 1000.0).sum();
    total / samples.len() as f64
}

fn percentile_ms(samples: &[Duration], percentile: usize) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut values = samples.iter().map(|s| s.as_secs_f64() * 1000.0).collect::<Vec<_>>();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = ((values.len() - 1) * percentile) / 100;
    values[rank]
}
