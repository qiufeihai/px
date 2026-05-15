#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use px_proto::{load_client_config, save_client_config, ClientConfig, TunConfig};
use px_runtime::{ClientRuntime, LogCallback};
use serde::{Deserialize, Serialize};
use tauri::State;
use std::fs;

struct AppState {
    runtime: Mutex<Option<ClientRuntime>>,
    tun_runtime: Mutex<Option<TunRuntime>>,
    logs: Arc<Mutex<String>>,
}

#[derive(Debug, Serialize)]
struct RuntimeState {
    running: bool,
    pid: Option<u32>,
    message: String,
}

#[derive(Debug, Serialize)]
struct RuntimePaths {
    runtime_dir: String,
    config_path: String,
    cert_path: String,
    config_exists: bool,
    cert_exists: bool,
}

#[derive(Debug, Serialize)]
struct TunState {
    running: bool,
    pid: Option<u32>,
    message: String,
}

struct TunRuntime {
    child: Child,
    route_plan: TunRoutePlan,
}

#[derive(Debug, Clone)]
struct TunRoutePlan {
    device_name: String,
    tun_ipv4: String,
    primary_interface: String,
    primary_gateway: String,
    server_ip: String,
}

#[derive(Debug, Serialize)]
struct DownloadTunHelperResult {
    helper_path: String,
    wintun_path: Option<String>,
    message: String,
}

#[tauri::command]
fn load_client_config_command() -> Result<ClientConfig, String> {
    let path = client_config_path().map_err(|error| error.to_string())?;
    load_client_config(&path).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_client_config_command(config: ClientConfig) -> Result<(), String> {
    let path = client_config_path().map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    save_client_config(&path, &config).map_err(|error| error.to_string())
}

#[tauri::command]
fn import_server_cert_command(source_path: String) -> Result<ClientConfig, String> {
    let source = PathBuf::from(source_path);
    if !source.exists() {
        return Err(format!("证书文件不存在: {}", source.display()));
    }

    let config_path = client_config_path().map_err(|error| error.to_string())?;
    let config_dir = config_path
        .parent()
        .ok_or_else(|| "无法定位当前运行目录下的 config 目录".to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|error| error.to_string())?;

    let target_path = config_dir.join("server-cert.pem");
    std::fs::copy(&source, &target_path).map_err(|error| {
        format!(
            "导入证书失败 {} -> {}: {error}",
            source.display(),
            target_path.display()
        )
    })?;

    let mut config = if config_path.exists() {
        load_client_config(&config_path).map_err(|error| error.to_string())?
    } else {
        default_client_config()
    };
    config.server_cert_path = "config/server-cert.pem".to_string();
    save_client_config(&config_path, &config).map_err(|error| error.to_string())?;
    Ok(config)
}

#[tauri::command]
fn runtime_paths_command() -> Result<RuntimePaths, String> {
    let runtime_dir = runtime_dir().map_err(|error| error.to_string())?;
    let config_path = client_config_path().map_err(|error| error.to_string())?;
    let cert_path = runtime_dir.join("config/server-cert.pem");
    Ok(RuntimePaths {
        runtime_dir: runtime_dir.display().to_string(),
        config_path: config_path.display().to_string(),
        cert_path: cert_path.display().to_string(),
        config_exists: config_path.exists(),
        cert_exists: cert_path.exists(),
    })
}

#[tauri::command]
fn open_config_dir_command() -> Result<(), String> {
    let runtime_dir = runtime_dir().map_err(|error| error.to_string())?;
    let config_dir = runtime_dir.join("config");
    std::fs::create_dir_all(&config_dir).map_err(|error| error.to_string())?;

    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(&config_dir).status()
    } else if cfg!(target_os = "windows") {
        Command::new("explorer").arg(&config_dir).status()
    } else {
        Command::new("xdg-open").arg(&config_dir).status()
    }
    .map_err(|error| error.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("打开配置目录失败: {}", config_dir.display()))
    }
}

#[tauri::command]
fn runtime_logs_command(state: State<'_, AppState>) -> Result<String, String> {
    let logs = state.logs.lock().map_err(|_| "state poisoned".to_string())?;
    Ok(logs.clone())
}

#[tauri::command]
fn clear_runtime_logs_command(state: State<'_, AppState>) -> Result<(), String> {
    clear_logs(&state.logs)
}

#[tauri::command]
async fn download_tun_helper_command(state: State<'_, AppState>) -> Result<DownloadTunHelperResult, String> {
    {
        let guard = state.tun_runtime.lock().map_err(|_| "state poisoned".to_string())?;
        if guard.is_some() {
            return Err("TUN 正在运行，请先停止 TUN，再更新 helper。".to_string());
        }
    }

    let runtime_dir = runtime_dir().map_err(|error| error.to_string())?;
    let logs = state.logs.clone();
    tokio::task::spawn_blocking(move || run_tun_helper_fetch_script(&runtime_dir, &logs))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn runtime_state(state: State<'_, AppState>) -> Result<RuntimeState, String> {
    let mut guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
    if let Some(runtime) = guard.as_ref() {
        if runtime.is_finished() {
            *guard = None;
            Ok(RuntimeState {
                running: false,
                pid: None,
                message: "客户端已退出，请查看最近日志。".to_string(),
            })
        } else {
            Ok(RuntimeState {
                running: true,
                pid: None,
                message: "客户端运行中".to_string(),
            })
        }
    } else {
        Ok(RuntimeState {
            running: false,
            pid: None,
            message: "客户端未启动".to_string(),
        })
    }
}

#[tauri::command]
fn tun_state(state: State<'_, AppState>) -> Result<TunState, String> {
    let mut guard = state.tun_runtime.lock().map_err(|_| "state poisoned".to_string())?;
    if let Some(runtime) = guard.as_mut() {
        match runtime.child.try_wait() {
            Ok(Some(status)) => {
                let pid = runtime.child.id();
                let route_plan = runtime.route_plan.clone();
                *guard = None;
                let _ = cleanup_tun_routes(&route_plan, &state.logs);
                Ok(TunState {
                    running: false,
                    pid: Some(pid),
                    message: format!("TUN helper 已退出，状态码 {}", status),
                })
            }
            Ok(None) => Ok(TunState {
                running: true,
                pid: Some(runtime.child.id()),
                message: "TUN 已运行".to_string(),
            }),
            Err(error) => Err(error.to_string()),
        }
    } else {
        Ok(TunState {
            running: false,
            pid: None,
            message: "TUN 未启动".to_string(),
        })
    }
}

#[tauri::command]
async fn start_client(state: State<'_, AppState>) -> Result<RuntimeState, String> {
    {
        let guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
        if guard.is_some() {
            return Ok(RuntimeState {
                running: true,
                pid: None,
                message: "客户端已在运行".to_string(),
            });
        }
    }

    let runtime_dir = runtime_dir().map_err(|error| error.to_string())?;
    let config_path = client_config_path().map_err(|error| error.to_string())?;
    let config = validate_client_start(&runtime_dir, &config_path)?;
    clear_logs(&state.logs)?;
    let logger = build_log_callback(state.logs.clone());
    let runtime = ClientRuntime::start(config.clone(), Some(logger))
        .await
        .map_err(|error| translate_runtime_start_error(&error, &config.local_socks_addr))?;

    let mut guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
    if guard.is_some() {
        return Ok(RuntimeState {
            running: true,
            pid: None,
            message: "客户端已在运行".to_string(),
        });
    }
    *guard = Some(runtime);
    Ok(RuntimeState {
        running: true,
        pid: None,
        message: "客户端已启动".to_string(),
    })
}

#[tauri::command]
async fn stop_client(state: State<'_, AppState>) -> Result<RuntimeState, String> {
    if let Err(error) = stop_tun_impl(&state) {
        append_log_line(&state.logs, "tun", &format!("停止客户端前清理 TUN 失败: {error}"));
    }

    let runtime = {
        let mut guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
        guard.take()
    };

    if let Some(runtime) = runtime {
        runtime.stop().await.map_err(|error| error.to_string())?;
        Ok(RuntimeState {
            running: false,
            pid: None,
            message: "客户端已停止".to_string(),
        })
    } else {
        Ok(RuntimeState {
            running: false,
            pid: None,
            message: "客户端未运行".to_string(),
        })
    }
}

#[tauri::command]
async fn start_tun(state: State<'_, AppState>) -> Result<TunState, String> {
    {
        let guard = state.tun_runtime.lock().map_err(|_| "state poisoned".to_string())?;
        if guard.is_some() {
            return Ok(TunState {
                running: true,
                pid: guard.as_ref().map(|runtime| runtime.child.id()),
                message: "TUN 已在运行".to_string(),
            });
        }
    }

    let runtime_dir = runtime_dir().map_err(|error| error.to_string())?;
    let config_path = client_config_path().map_err(|error| error.to_string())?;
    let config = validate_client_start(&runtime_dir, &config_path)?;
    let route_plan = validate_tun_start(&runtime_dir, &config)?;
    ensure_client_runtime_running(&state, &config).await?;

    let mut child = spawn_tun_helper(&runtime_dir, &config, &route_plan.primary_interface, &state.logs)?;
    std::thread::sleep(Duration::from_millis(800));
    if let Err(error) = setup_tun_routes(&route_plan, &state.logs) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let pid = child.id();

    let mut guard = state.tun_runtime.lock().map_err(|_| "state poisoned".to_string())?;
    if guard.is_some() {
        return Ok(TunState {
            running: true,
            pid: Some(pid),
            message: "TUN 已在运行".to_string(),
        });
    }
    *guard = Some(TunRuntime { child, route_plan });
    Ok(TunState {
        running: true,
        pid: Some(pid),
        message: "TUN 已启动".to_string(),
    })
}

#[tauri::command]
async fn stop_tun(state: State<'_, AppState>) -> Result<TunState, String> {
    let runtime = {
        let mut guard = state.tun_runtime.lock().map_err(|_| "state poisoned".to_string())?;
        guard.take()
    };

    if let Some(mut runtime) = runtime {
        cleanup_tun_routes(&runtime.route_plan, &state.logs)?;
        let pid = runtime.child.id();
        runtime.child.kill().map_err(|error| error.to_string())?;
        let _ = runtime.child.wait();
        append_log_line(&state.logs, "tun", "TUN helper 已停止。");
        Ok(TunState {
            running: false,
            pid: Some(pid),
            message: "TUN 已停止".to_string(),
        })
    } else {
        Ok(TunState {
            running: false,
            pid: None,
            message: "TUN 未运行".to_string(),
        })
    }
}


#[tauri::command]
async fn test_proxy_connectivity() -> Result<String, String> {
    let config_path = client_config_path().map_err(|error| error.to_string())?;
    let config = load_client_config(&config_path).map_err(|_| "读取客户端配置失败，请先保存配置。".to_string())?;
    let timeout = Duration::from_secs(5);
    let mut stream = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&config.local_socks_addr))
        .await
        .map_err(|_| "连接本地 SOCKS5 超时".to_string())
        .and_then(|result| {
            result.map_err(|error| translate_socks_connect_error(&error, &config.local_socks_addr))
        })?;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    stream
        .write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|error| error.to_string())?;
    let mut auth = [0_u8; 2];
    stream
        .read_exact(&mut auth)
        .await
        .map_err(|error| error.to_string())?;
    if auth != [0x05, 0x00] {
        return Err("SOCKS5 鉴权协商失败".to_string());
    }

    let domain = b"example.com";
    stream
        .write_all(&[0x05, 0x01, 0x00, 0x03, domain.len() as u8])
        .await
        .map_err(|error| error.to_string())?;
    stream.write_all(domain).await.map_err(|error| error.to_string())?;
    stream.write_u16(80).await.map_err(|error| error.to_string())?;
    let mut response = [0_u8; 10];
    stream
        .read_exact(&mut response)
        .await
        .map_err(|error| error.to_string())?;
    if response[1] != 0x00 {
        return Err(format!("代理链路失败，SOCKS5 返回码 {}", response[1]));
    }

    Ok("代理链路正常，已通过本地 SOCKS5 成功连通 example.com:80。".to_string())
}

fn client_config_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("config/client.toml"))
}

fn runtime_dir() -> Result<PathBuf> {
    Ok(std::env::current_dir()?)
}

fn validate_client_start(runtime_dir: &Path, config_path: &Path) -> Result<ClientConfig, String> {
    if !config_path.exists() {
        return Err("当前运行目录下缺少 config/client.toml，请先在界面保存配置。".to_string());
    }

    let config = load_client_config(config_path).map_err(|_| "读取客户端配置失败，请重新保存配置。".to_string())?;
    if config.server_addr.trim().is_empty() {
        return Err("服务端地址为空，请先填写服务端地址。".to_string());
    }

    let cert_path = resolve_runtime_path(runtime_dir, &config.server_cert_path);
    if !cert_path.exists() {
        return Err("未找到服务端证书，请先点击“导入证书”或检查证书路径。".to_string());
    }

    Ok(config)
}

fn validate_tun_start(runtime_dir: &Path, config: &ClientConfig) -> Result<TunRoutePlan, String> {
    if !config.tun.enabled {
        return Err("TUN 未启用，请先勾选“启用 TUN 全局 TCP”。".to_string());
    }

    let helper_path = resolve_runtime_path(runtime_dir, &config.tun.helper_path);
    if !helper_path.exists() {
        return Err(format!(
            "未找到 TUN helper: {}。请先点击“下载 helper”，或把 tun2socks 放到当前运行目录的 bin/ 中。",
            helper_path.display()
        ));
    }
    if cfg!(target_os = "windows") {
        let wintun_path = helper_path
            .parent()
            .unwrap_or(runtime_dir)
            .join("wintun.dll");
        if !wintun_path.exists() {
            return Err(format!(
                "未找到 wintun.dll: {}。请先点击“下载 helper”，或把官方 wintun.dll 放到当前运行目录的 bin/ 中。",
                wintun_path.display()
            ));
        }
    }

    let server = config
        .server_addr
        .parse::<SocketAddr>()
        .map_err(|_| "服务端地址格式错误，请填写 IP:端口。".to_string())?;
    let (default_interface, default_gateway) = detect_default_route()?;
    let primary_interface = if config.tun.primary_interface.trim().is_empty() {
        default_interface
    } else {
        config.tun.primary_interface.trim().to_string()
    };

    Ok(TunRoutePlan {
        device_name: config.tun.device_name.trim().to_string(),
        tun_ipv4: config.tun.ipv4_addr.trim().to_string(),
        primary_interface,
        primary_gateway: default_gateway,
        server_ip: server.ip().to_string(),
    })
}

async fn ensure_client_runtime_running(state: &State<'_, AppState>, config: &ClientConfig) -> Result<(), String> {
    {
        let guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
        if guard.is_some() {
            return Ok(());
        }
    }

    let logger = build_log_callback(state.logs.clone());
    let runtime = ClientRuntime::start(config.clone(), Some(logger))
        .await
        .map_err(|error| translate_runtime_start_error(&error, &config.local_socks_addr))?;

    let mut guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
    if guard.is_none() {
        *guard = Some(runtime);
    }
    Ok(())
}

fn resolve_runtime_path(runtime_dir: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        runtime_dir.join(path)
    }
}

fn spawn_tun_helper(
    runtime_dir: &Path,
    config: &ClientConfig,
    primary_interface: &str,
    logs: &Arc<Mutex<String>>,
) -> Result<Child, String> {
    let helper_path = resolve_runtime_path(runtime_dir, &config.tun.helper_path);
    let mut command = Command::new(&helper_path);
    command
        .current_dir(runtime_dir)
        .arg("-device")
        .arg(&config.tun.device_name)
        .arg("-proxy")
        .arg(format!("socks5://{}", config.local_socks_addr))
        .arg("-interface")
        .arg(primary_interface)
        .arg("-mtu")
        .arg(config.tun.mtu.to_string())
        .arg("-loglevel")
        .arg(map_tun_log_level(&config.log_level))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = command.spawn().map_err(|error| error.to_string())?;
    append_log_line(
        logs,
        "tun",
        &format!(
            "已启动 helper: {} -> socks5://{} ({})",
            helper_path.display(),
            config.local_socks_addr,
            format_tun_summary(&config.tun)
        ),
    );
    Ok(child)
}

fn helper_relative_path() -> &'static str {
    if cfg!(target_os = "windows") {
        "bin/tun2socks.exe"
    } else {
        "bin/tun2socks"
    }
}

fn run_tun_helper_fetch_script(runtime_dir: &Path, logs: &Arc<Mutex<String>>) -> Result<DownloadTunHelperResult, String> {
    let script_path = resolve_fetch_tun_helper_script(runtime_dir)?;
    let bin_dir = runtime_dir.join("bin");
    fs::create_dir_all(&bin_dir).map_err(|error| error.to_string())?;

    append_log_line(logs, "tun", &format!("开始下载 helper: {}", script_path.display()));
    let status = if cfg!(target_os = "windows") {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script_path)
            .arg("-BinDir")
            .arg(&bin_dir)
            .current_dir(runtime_dir)
            .status()
    } else {
        Command::new("bash")
            .arg(&script_path)
            .env("BIN_DIR", &bin_dir)
            .current_dir(runtime_dir)
            .status()
    }
    .map_err(|error| error.to_string())?;

    if !status.success() {
        return Err("下载 helper 失败，请检查网络、脚本权限或下载源可达性。".to_string());
    }

    let helper_path = bin_dir.join(Path::new(helper_relative_path()).file_name().unwrap_or_default());
    if !helper_path.exists() {
        return Err(format!("下载完成后仍未找到 helper: {}", helper_path.display()));
    }

    let wintun_path = if cfg!(target_os = "windows") {
        let path = bin_dir.join("wintun.dll");
        if !path.exists() {
            return Err(format!("下载完成后仍未找到 wintun.dll: {}", path.display()));
        }
        Some("bin/wintun.dll".to_string())
    } else {
        None
    };

    append_log_line(logs, "tun", &format!("helper 已就绪: {}", helper_path.display()));
    Ok(DownloadTunHelperResult {
        helper_path: helper_relative_path().to_string(),
        wintun_path,
        message: "TUN helper 已下载到当前运行目录的 bin/。".to_string(),
    })
}

fn resolve_fetch_tun_helper_script(runtime_dir: &Path) -> Result<PathBuf, String> {
    let candidates = if cfg!(target_os = "windows") {
        vec![
            runtime_dir.join("scripts/fetch-tun-helper.ps1"),
            runtime_dir.join("../../scripts/fetch-tun-helper.ps1"),
        ]
    } else {
        vec![
            runtime_dir.join("scripts/fetch-tun-helper.sh"),
            runtime_dir.join("../../scripts/fetch-tun-helper.sh"),
        ]
    };

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("未找到 fetch-tun-helper 脚本，请确认当前运行目录是发布目录，或在开发环境从 apps/tauri-ui 启动 GUI。".to_string())
}

#[derive(Debug, Deserialize)]
struct WindowsDefaultRoute {
    #[serde(rename = "InterfaceAlias")]
    interface_alias: String,
    #[serde(rename = "NextHop")]
    next_hop: String,
}

fn detect_default_route() -> Result<(String, String), String> {
    if cfg!(target_os = "macos") {
        let output = Command::new("route")
            .args(["-n", "get", "default"])
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err("读取系统默认路由失败。".to_string());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut interface = String::new();
        let mut gateway = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("interface:") {
                interface = value.trim().to_string();
            }
            if let Some(value) = trimmed.strip_prefix("gateway:") {
                gateway = value.trim().to_string();
            }
        }
        if interface.is_empty() || gateway.is_empty() {
            return Err("解析系统默认路由失败。".to_string());
        }
        return Ok((interface, gateway));
    }

    if cfg!(target_os = "windows") {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' | Sort-Object RouteMetric,InterfaceMetric | Select-Object -First 1 InterfaceAlias,NextHop | ConvertTo-Json -Compress",
            ])
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err("读取系统默认路由失败。".to_string());
        }
        let route: WindowsDefaultRoute =
            serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
        if route.interface_alias.trim().is_empty() || route.next_hop.trim().is_empty() {
            return Err("解析系统默认路由失败。".to_string());
        }
        return Ok((route.interface_alias, route.next_hop));
    }

    Err("当前平台暂未接入 TUN。".to_string())
}

fn setup_tun_routes(plan: &TunRoutePlan, logs: &Arc<Mutex<String>>) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        run_command_checked("ifconfig", &[&plan.device_name, &plan.tun_ipv4, &plan.tun_ipv4, "up"])?;
        run_command_checked("route", &["-n", "add", "-host", &plan.server_ip, &plan.primary_gateway])?;
        for cidr in macos_tun_routes() {
            run_command_checked("route", &["-n", "add", "-net", cidr, &plan.tun_ipv4])?;
        }
    } else if cfg!(target_os = "windows") {
        run_command_checked(
            "netsh",
            &[
                "interface",
                "ipv4",
                "set",
                "address",
                &format!("name={}", plan.device_name),
                "source=static",
                &format!("addr={}", plan.tun_ipv4),
                "mask=255.255.255.0",
            ],
        )?;
        run_command_checked(
            "route",
            &["ADD", &plan.server_ip, "MASK", "255.255.255.255", &plan.primary_gateway],
        )?;
        run_command_checked(
            "netsh",
            &[
                "interface",
                "ipv4",
                "add",
                "route",
                "0.0.0.0/0",
                &plan.device_name,
                &plan.tun_ipv4,
                "metric=1",
            ],
        )?;
    } else {
        return Err("当前平台暂未接入 TUN。".to_string());
    }

    append_log_line(
        logs,
        "tun",
        &format!(
            "TUN 路由已生效: server={} 走 {}，其余 TCP 走 {}",
            plan.server_ip, plan.primary_interface, plan.device_name
        ),
    );
    Ok(())
}

fn cleanup_tun_routes(plan: &TunRoutePlan, logs: &Arc<Mutex<String>>) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        run_command_best_effort("route", &["-n", "delete", "-host", &plan.server_ip, &plan.primary_gateway]);
        for cidr in macos_tun_routes() {
            run_command_best_effort("route", &["-n", "delete", "-net", cidr, &plan.tun_ipv4]);
        }
        run_command_best_effort("ifconfig", &[&plan.device_name, "down"]);
    } else if cfg!(target_os = "windows") {
        run_command_best_effort(
            "netsh",
            &[
                "interface",
                "ipv4",
                "delete",
                "route",
                "0.0.0.0/0",
                &plan.device_name,
                &plan.tun_ipv4,
            ],
        );
        run_command_best_effort(
            "route",
            &["DELETE", &plan.server_ip, "MASK", "255.255.255.255", &plan.primary_gateway],
        );
    } else {
        return Err("当前平台暂未接入 TUN。".to_string());
    }

    append_log_line(logs, "tun", "TUN 路由已清理。");
    Ok(())
}

fn stop_tun_impl(state: &State<'_, AppState>) -> Result<(), String> {
    let runtime = {
        let mut guard = state.tun_runtime.lock().map_err(|_| "state poisoned".to_string())?;
        guard.take()
    };

    if let Some(mut runtime) = runtime {
        cleanup_tun_routes(&runtime.route_plan, &state.logs)?;
        let _ = runtime.child.kill();
        let _ = runtime.child.wait();
    }
    Ok(())
}

fn run_command_checked(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} 执行失败: {}", args.join(" ")))
    }
}

fn run_command_best_effort(program: &str, args: &[&str]) {
    let _ = Command::new(program).args(args).status();
}

fn macos_tun_routes() -> &'static [&'static str] {
    &[
        "1.0.0.0/8",
        "2.0.0.0/7",
        "4.0.0.0/6",
        "8.0.0.0/5",
        "16.0.0.0/4",
        "32.0.0.0/3",
        "64.0.0.0/2",
        "128.0.0.0/1",
        "198.18.0.0/15",
    ]
}

fn format_tun_summary(tun: &TunConfig) -> String {
    format!(
        "device={}, interface={}, ipv4={}, mtu={}",
        tun.device_name, tun.primary_interface, tun.ipv4_addr, tun.mtu
    )
}

fn map_tun_log_level(value: &str) -> &'static str {
    match value {
        "error" => "error",
        "warn" => "warn",
        "debug" => "debug",
        "trace" => "debug",
        _ => "info",
    }
}

fn translate_runtime_start_error(error: &anyhow::Error, addr: &str) -> String {
    let text = error.to_string();
    if text.contains("Address already in use") || text.contains("failed to bind") {
        return format!("本地 SOCKS5 端口已被占用，请更换 {addr} 或先停止占用该端口的程序。");
    }
    if text.contains("Permission denied") {
        return format!("没有权限监听本地地址 {addr}。");
    }
    if text.contains("server_addr must be IP:port") {
        return "服务端地址格式错误，请填写 IP:端口。".to_string();
    }
    format!("启动客户端失败: {text}")
}

fn translate_socks_connect_error(error: &std::io::Error, addr: &str) -> String {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => {
            format!("本地 SOCKS5 未监听 {addr}，请先启动客户端。")
        }
        std::io::ErrorKind::TimedOut => {
            format!("连接本地 SOCKS5 超时: {addr}。")
        }
        std::io::ErrorKind::NotFound => {
            "未找到本地 SOCKS5 地址。".to_string()
        }
        _ => format!("连接本地 SOCKS5 失败 {addr}: {error}"),
    }
}

fn clear_logs(logs: &Arc<Mutex<String>>) -> Result<(), String> {
    let mut buffer = logs.lock().map_err(|_| "state poisoned".to_string())?;
    buffer.clear();
    Ok(())
}

fn build_log_callback(logs: Arc<Mutex<String>>) -> LogCallback {
    Arc::new(move |line| append_log_line(&logs, "runtime", &line))
}

fn append_log_line(logs: &Arc<Mutex<String>>, stream_name: &str, line: &str) {
    const LOG_LIMIT: usize = 16 * 1024;

    if let Ok(mut buffer) = logs.lock() {
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(&format!("[{stream_name}] {line}"));

        if buffer.len() > LOG_LIMIT {
            let start = buffer.len().saturating_sub(LOG_LIMIT);
            let trimmed = buffer[start..].to_string();
            *buffer = trimmed;
        }
    }
}

fn ensure_default_config() {
    let path = match client_config_path() {
        Ok(path) => path,
        Err(_) => return,
    };
    if path.exists() {
        return;
    }

    let default = default_client_config();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let _ = save_client_config(&path, &default);
}

fn default_client_config() -> ClientConfig {
    ClientConfig {
        server_addr: "127.0.0.1:6666".to_string(),
        server_cert_path: "config/server-cert.pem".to_string(),
        local_socks_addr: "127.0.0.1:7777".to_string(),
        connect_timeout_ms: 5000,
        log_level: "info".to_string(),
        tun: TunConfig::default(),
    }
}

fn main() {
    ensure_default_config();
    tauri::Builder::default()
        .manage(AppState {
            runtime: Mutex::new(None),
            tun_runtime: Mutex::new(None),
            logs: Arc::new(Mutex::new(String::new())),
        })
        .invoke_handler(tauri::generate_handler![
            load_client_config_command,
            save_client_config_command,
            import_server_cert_command,
            runtime_paths_command,
            runtime_logs_command,
            clear_runtime_logs_command,
            download_tun_helper_command,
            open_config_dir_command,
            runtime_state,
            tun_state,
            start_client,
            stop_client,
            start_tun,
            stop_tun,
            test_proxy_connectivity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
