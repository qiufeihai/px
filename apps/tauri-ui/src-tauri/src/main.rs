#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use px_proto::{
    load_client_config, save_client_config, ClientConfig, ConnectRequest, ConnectResponse,
    StatusCode, TargetAddr, TunConfig,
};
use px_runtime::{ClientRuntime, LogCallback};
use serde::{Deserialize, Serialize};
use tauri::State;
use std::fs;

struct AppState {
    runtime: Mutex<Option<ClientRuntimes>>,
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
    process: TunProcess,
    route_plan: TunRoutePlan,
    runtime_dir: PathBuf,
}

struct ClientRuntimes {
    primary: ClientRuntime,
    ingress: Option<ClientRuntime>,
}

struct LocalProxyEndpoint {
    addr: String,
    helper_proxy_arg: String,
    display: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TunHelperKind {
    Tun2Socks,
    PxTunHelper,
}

enum TunProcess {
    Local(Child),
    Privileged { pid: u32 },
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
struct RepairTunHelperResult {
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
async fn repair_tun_helper_command(state: State<'_, AppState>) -> Result<RepairTunHelperResult, String> {
    {
        let guard = state.tun_runtime.lock().map_err(|_| "state poisoned".to_string())?;
        if guard.is_some() {
            return Err("TUN 正在运行，请先停止 TUN，再更新 helper。".to_string());
        }
    }

    let runtime_dir = runtime_dir().map_err(|error| error.to_string())?;
    let logs = state.logs.clone();
    tokio::task::spawn_blocking(move || run_tun_helper_repair_script(&runtime_dir, &logs))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn runtime_state(state: State<'_, AppState>) -> Result<RuntimeState, String> {
    let mut guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
    if let Some(runtimes) = guard.as_mut() {
        if runtimes.primary.is_finished() {
            *guard = None;
            Ok(RuntimeState {
                running: false,
                pid: None,
                message: "客户端已退出，请查看最近日志。".to_string(),
            })
        } else {
            if let Some(ingress) = runtimes.ingress.as_ref() {
                if ingress.is_finished() {
                    runtimes.ingress = None;
                    append_log_line(
                        &state.logs,
                        "runtime",
                        "预留 ingress listener 已退出，当前仍保留 SOCKS5 主路径。",
                    );
                }
            }
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
    if guard.is_none() {
        if let Some(runtime) = recover_tun_runtime()? {
            *guard = Some(runtime);
        }
    }
    if let Some(runtime) = guard.as_mut() {
        let pid = tun_process_pid(&runtime.process);
        if tun_process_is_running(&mut runtime.process)? {
            return Ok(TunState {
                running: true,
                pid: Some(pid),
                message: "TUN 已运行".to_string(),
            });
        }

        let runtime = guard.take().ok_or_else(|| "state poisoned".to_string())?;
        if matches!(runtime.process, TunProcess::Local(_)) {
            let _ = cleanup_tun_routes(&runtime.route_plan, &state.logs);
        }
        Ok(TunState {
            running: false,
            pid: Some(pid),
            message: "TUN helper 已退出，请查看最近日志。".to_string(),
        })
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
    let runtimes = start_default_client_runtimes(state.logs.clone(), config.clone()).await?;

    let mut pending_runtimes = Some(runtimes);
    let already_running = {
        let mut guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
        if guard.is_some() {
            true
        } else {
            *guard = pending_runtimes.take();
            false
        }
    };
    if already_running {
        stop_client_runtimes(
            pending_runtimes
                .take()
                .ok_or_else(|| "state poisoned".to_string())?,
        )
        .await?;
        return Ok(RuntimeState {
            running: true,
            pid: None,
            message: "客户端已在运行".to_string(),
        });
    }
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

    let runtimes = {
        let mut guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
        guard.take()
    };

    if let Some(runtimes) = runtimes {
        stop_client_runtimes(runtimes).await?;
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
                pid: guard.as_ref().map(|runtime| tun_process_pid(&runtime.process)),
                message: "TUN 已在运行".to_string(),
            });
        }
    }

    let runtime_dir = runtime_dir().map_err(|error| error.to_string())?;
    let config_path = client_config_path().map_err(|error| error.to_string())?;
    let config = validate_client_start(&runtime_dir, &config_path)?;
    let route_plan = validate_tun_start(&runtime_dir, &config)?;
    ensure_client_runtime_running(&state, &config).await?;
    ensure_tun_ingress_ready(&state, &runtime_dir, &config)?;

    let process = if cfg!(target_os = "macos") {
        let runtime_dir_for_task = runtime_dir.clone();
        let config_for_task = config.clone();
        let route_plan_for_task = route_plan.clone();
        let logs_for_task = state.logs.clone();
        tokio::task::spawn_blocking(move || {
            start_macos_privileged_tun(
                &runtime_dir_for_task,
                &config_for_task,
                &route_plan_for_task,
                &logs_for_task,
            )
        })
        .await
        .map_err(|error| error.to_string())??
    } else {
        let mut child = spawn_local_tun_helper(&runtime_dir, &config, &route_plan.primary_interface, &state.logs)?;
        std::thread::sleep(Duration::from_millis(800));
        if let Err(error) = setup_tun_routes(&route_plan, &state.logs) {
            append_tun_helper_log_tail(&runtime_dir, &state.logs);
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        TunProcess::Local(child)
    };
    let pid = tun_process_pid(&process);

    let mut guard = state.tun_runtime.lock().map_err(|_| "state poisoned".to_string())?;
    if guard.is_some() {
        return Ok(TunState {
            running: true,
            pid: Some(pid),
            message: "TUN 已在运行".to_string(),
        });
    }
    *guard = Some(TunRuntime {
        process,
        route_plan,
        runtime_dir,
    });
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
        if guard.is_none() {
            if let Some(runtime) = recover_tun_runtime()? {
                *guard = Some(runtime);
            }
        }
        guard.take()
    };

    if let Some(runtime) = runtime {
        let pid = tun_process_pid(&runtime.process);
        stop_tun_runtime(runtime, &state.logs)?;
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
    let proxy = current_local_proxy_endpoint(&config);
    let timeout = Duration::from_secs(5);
    let mut stream = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&proxy.addr))
        .await
        .map_err(|_| "连接本地 SOCKS5 超时".to_string())
        .and_then(|result| {
            result.map_err(|error| translate_socks_connect_error(&error, &proxy.addr))
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
    if cfg!(debug_assertions) {
        let cwd = std::env::current_dir()?;
        let base_dir = if cwd.file_name().and_then(|name| name.to_str()) == Some("src-tauri") {
            cwd.parent().unwrap_or(&cwd)
        } else {
            &cwd
        };
        return Ok(base_dir.join(".px-dev-runtime"));
    }
    resolve_release_runtime_dir()
}

fn resolve_release_runtime_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing executable parent"))?;
    let mut candidates = vec![exe_dir.to_path_buf()];

    if let Some(parent) = exe_dir.parent() {
        candidates.push(parent.to_path_buf());
        if let Some(grand_parent) = parent.parent() {
            candidates.push(grand_parent.to_path_buf());
        }
    }

    if cfg!(target_os = "macos")
        && exe_dir.file_name().and_then(|name| name.to_str()) == Some("MacOS")
    {
        if let Some(contents_dir) = exe_dir.parent() {
            if let Some(app_dir) = contents_dir.parent() {
                if let Some(bundle_parent) = app_dir.parent() {
                    candidates.push(bundle_parent.to_path_buf());
                    if let Some(root_parent) = bundle_parent.parent() {
                        candidates.push(root_parent.to_path_buf());
                    }
                }
            }
        }
    }

    candidates.dedup();

    if let Some(dir) = candidates.into_iter().find(|dir| {
        dir.join("config").is_dir() || dir.join("bin").is_dir() || dir.join("scripts").is_dir()
    }) {
        return Ok(dir);
    }

    Ok(exe_dir.to_path_buf())
}

fn validate_client_start(runtime_dir: &Path, config_path: &Path) -> Result<ClientConfig, String> {
    if !config_path.exists() {
        return Err("当前运行目录下缺少 config/client.toml，请先在界面保存配置。".to_string());
    }

    let mut config = load_client_config(config_path).map_err(|_| "读取客户端配置失败，请重新保存配置。".to_string())?;
    if config.server_addr.trim().is_empty() {
        return Err("服务端地址为空，请先填写服务端地址。".to_string());
    }

    let cert_path = resolve_runtime_path(runtime_dir, &config.server_cert_path);
    if !cert_path.exists() {
        return Err("未找到服务端证书，请先点击“导入证书”或检查证书路径。".to_string());
    }
    config.server_cert_path = cert_path.display().to_string();

    Ok(config)
}

fn validate_tun_start(runtime_dir: &Path, config: &ClientConfig) -> Result<TunRoutePlan, String> {
    if !config.tun.enabled {
        return Err("TUN 未启用，请先勾选“启用 TUN 全局 TCP”。".to_string());
    }

    let helper_path = resolve_runtime_path(runtime_dir, &config.tun.helper_path);
    if !helper_path.exists() {
        return Err(format!(
            "未找到 TUN helper: {}。请先点击“修复 helper”，或把 helper 放到当前运行目录的 bin/ 中。",
            helper_path.display()
        ));
    }
    let helper_kind = detect_tun_helper_kind(&helper_path);
    if cfg!(target_os = "windows") {
        let wintun_path = helper_path
            .parent()
            .unwrap_or(runtime_dir)
            .join("wintun.dll");
        if !wintun_path.exists() {
            return Err(format!(
                "未找到 wintun.dll: {}。请先点击“修复 helper”，或把官方 wintun.dll 放到当前运行目录的 bin/ 中。",
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
    if helper_kind == TunHelperKind::PxTunHelper {
        if cfg!(target_os = "macos") && !is_explicit_utun_name(&config.tun.device_name) {
            return Err("px-tun-helper 当前要求显式 utun 设备名，例如 utun233，避免真实设备与路由清理错位。".to_string());
        }
    }

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

    let runtimes = start_default_client_runtimes(state.logs.clone(), config.clone()).await?;

    let mut pending_runtimes = Some(runtimes);
    let already_running = {
        let mut guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
        if guard.is_none() {
            *guard = pending_runtimes.take();
            false
        } else {
            true
        }
    };
    if already_running {
        stop_client_runtimes(
            pending_runtimes
                .take()
                .ok_or_else(|| "state poisoned".to_string())?,
        )
        .await?;
    }
    Ok(())
}

fn ensure_tun_ingress_ready(
    state: &State<'_, AppState>,
    runtime_dir: &Path,
    config: &ClientConfig,
) -> Result<(), String> {
    let helper_path = resolve_runtime_path(runtime_dir, &config.tun.helper_path);
    if detect_tun_helper_kind(&helper_path) != TunHelperKind::PxTunHelper {
        return Ok(());
    }

    let guard = state.runtime.lock().map_err(|_| "state poisoned".to_string())?;
    let ingress_ready = guard
        .as_ref()
        .and_then(|runtimes| runtimes.ingress.as_ref())
        .map(|ingress| !ingress.is_finished())
        .unwrap_or(false);
    if ingress_ready {
        return Ok(());
    }

    Err("px-tun-helper 需要本地预留 ingress listener，但当前 ingress 未启动，请先重启客户端。".to_string())
}

async fn stop_client_runtimes(runtimes: ClientRuntimes) -> Result<(), String> {
    if let Some(ingress) = runtimes.ingress {
        ingress.stop().await.map_err(|error| error.to_string())?;
    }
    runtimes
        .primary
        .stop()
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn spawn_shadow_ingress_probe(logs: Arc<Mutex<String>>, ingress_addr: String) {
    tokio::spawn(async move {
        match probe_shadow_ingress(&ingress_addr).await {
            Ok(()) => append_log_line(
                &logs,
                "runtime",
                &format!("预留 ingress 自检成功: {ingress_addr} -> example.com:80"),
            ),
            Err(error) => append_log_line(
                &logs,
                "runtime",
                &format!(
                    "预留 ingress 自检失败，不影响当前 SOCKS5 主路径: {ingress_addr}: {error}"
                ),
            ),
        }
    });
}

async fn probe_shadow_ingress(ingress_addr: &str) -> Result<(), String> {
    probe_ingress_target(ingress_addr, "example.com", 80).await
}

async fn probe_ingress_target(ingress_addr: &str, host: &str, port: u16) -> Result<(), String> {
    let timeout = Duration::from_secs(5);
    let mut stream = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(ingress_addr))
        .await
        .map_err(|_| format!("连接预留 ingress 超时: {ingress_addr}"))?
        .map_err(|error| format!("连接预留 ingress 失败 {ingress_addr}: {error}"))?;

    let request = ConnectRequest {
        target: TargetAddr::Domain(host.to_string()),
        port,
    };
    request
        .write_to(&mut stream)
        .await
        .map_err(|error| format!("写入 ingress 请求失败: {error}"))?;

    let response = ConnectResponse::read_from(&mut stream)
        .await
        .map_err(|error| format!("读取 ingress 响应失败: {error}"))?;

    if response.status != StatusCode::Ok {
        return Err(format!(
            "ingress 返回状态 {:?}，reason={}, target={host}:{port}",
            response.status, response.reason
        ));
    }

    Ok(())
}

async fn start_default_client_runtimes(
    logs: Arc<Mutex<String>>,
    config: ClientConfig,
) -> Result<ClientRuntimes, String> {
    let proxy = current_local_proxy_endpoint(&config);
    let ingress_addr = suggested_ingress_bind_addr(&config);
    append_log_line(
        &logs,
        "runtime",
        &format!(
            "默认本地入口: {}；预留 ingress: {}",
            proxy.display,
            ingress_addr
        ),
    );
    let logger = build_log_callback(logs.clone());
    let primary = ClientRuntime::start_socks5(config.clone(), Some(logger))
        .await
        .map_err(|error| translate_runtime_start_error(&error, &proxy.addr))?;

    let ingress = match ClientRuntime::start_ingress(
        &ingress_addr,
        config,
        Some(build_log_callback(logs.clone())),
    )
    .await
    {
        Ok(runtime) => {
            append_log_line(
                &logs,
                "runtime",
                &format!("预留 ingress listener 已启动: {ingress_addr}"),
            );
            spawn_shadow_ingress_probe(logs.clone(), ingress_addr.clone());
            Some(runtime)
        }
        Err(error) => {
            append_log_line(
                &logs,
                "runtime",
                &format!(
                    "预留 ingress listener 启动失败，继续保持 SOCKS5 主路径: {ingress_addr}: {error}"
                ),
            );
            None
        }
    };

    Ok(ClientRuntimes { primary, ingress })
}

fn current_local_proxy_endpoint(config: &ClientConfig) -> LocalProxyEndpoint {
    let addr = config.local_socks_addr.clone();
    let helper_proxy_arg = format!("socks5://{addr}");
    LocalProxyEndpoint {
        addr,
        display: helper_proxy_arg.clone(),
        helper_proxy_arg,
    }
}

fn suggested_ingress_bind_addr(config: &ClientConfig) -> String {
    match config.local_socks_addr.parse::<SocketAddr>() {
        Ok(addr) if addr.port() < u16::MAX => SocketAddr::new(addr.ip(), addr.port() + 1).to_string(),
        Ok(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7778).to_string(),
        Err(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7778).to_string(),
    }
}

fn tun_helper_log_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("logs/tun-helper.log")
}

fn tun_helper_pid_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("logs/tun-helper.pid")
}

fn read_running_pid_from_file(pid_path: &Path) -> Result<Option<u32>, String> {
    if !pid_path.exists() {
        return Ok(None);
    }
    let pid_text = fs::read_to_string(pid_path).map_err(|error| error.to_string())?;
    let pid = pid_text
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("无效的 TUN pid 文件: {}", pid_path.display()))?;
    if is_process_running(pid) {
        Ok(Some(pid))
    } else {
        Ok(None)
    }
}

fn resolve_runtime_path(runtime_dir: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        runtime_dir.join(path)
    }
}

fn detect_tun_helper_kind(path: &Path) -> TunHelperKind {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("px-tun-helper") | Some("px-tun-helper.exe") => TunHelperKind::PxTunHelper,
        _ => TunHelperKind::Tun2Socks,
    }
}

fn is_explicit_utun_name(device_name: &str) -> bool {
    device_name
        .strip_prefix("utun")
        .map(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
        .unwrap_or(false)
}

fn spawn_local_tun_helper(
    runtime_dir: &Path,
    config: &ClientConfig,
    primary_interface: &str,
    logs: &Arc<Mutex<String>>,
) -> Result<Child, String> {
    let helper_path = resolve_runtime_path(runtime_dir, &config.tun.helper_path);
    let helper_kind = detect_tun_helper_kind(&helper_path);
    let helper_log_path = tun_helper_log_path(runtime_dir);
    if let Some(parent) = helper_log_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&helper_log_path)
        .map_err(|error| error.to_string())?;
    let stderr = stdout.try_clone().map_err(|error| error.to_string())?;
    let mut command = Command::new(&helper_path);
    command.current_dir(runtime_dir);
    let helper_target_display = match helper_kind {
        TunHelperKind::PxTunHelper => {
            let ingress_addr = suggested_ingress_bind_addr(config);
            command
                .arg("-device")
                .arg(&config.tun.device_name)
                .arg("-tun-ipv4")
                .arg(&config.tun.ipv4_addr)
                .arg("-ingress")
                .arg(&ingress_addr)
                .arg("-primary-interface")
                .arg(primary_interface)
                .arg("-mtu")
                .arg(config.tun.mtu.to_string())
                .arg("-connect-timeout-ms")
                .arg(config.connect_timeout_ms.to_string())
                .arg("-log-level")
                .arg(map_tun_log_level(&config.log_level));
            format!("ingress://{ingress_addr}")
        }
        TunHelperKind::Tun2Socks => {
            let proxy = current_local_proxy_endpoint(config);
            command
                .arg("-device")
                .arg(&config.tun.device_name)
                .arg("-proxy")
                .arg(&proxy.helper_proxy_arg)
                .arg("-interface")
                .arg(primary_interface)
                .arg("-mtu")
                .arg(config.tun.mtu.to_string())
                .arg("-loglevel")
                .arg(map_tun_log_level(&config.log_level));
            proxy.display
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    let child = command.spawn().map_err(|error| error.to_string())?;
    append_log_line(
        logs,
        "tun",
        &format!(
            "已启动 helper: {} -> {} ({})，日志: {}",
            helper_path.display(),
            helper_target_display,
            format_tun_summary(&config.tun),
            helper_log_path.display()
        ),
    );
    Ok(child)
}

fn start_macos_privileged_tun(
    runtime_dir: &Path,
    config: &ClientConfig,
    route_plan: &TunRoutePlan,
    logs: &Arc<Mutex<String>>,
) -> Result<TunProcess, String> {
    let proxy = current_local_proxy_endpoint(config);
    let helper_path = resolve_runtime_path(runtime_dir, &config.tun.helper_path);
    let helper_kind = detect_tun_helper_kind(&helper_path);
    let proxy_mode = match helper_kind {
        TunHelperKind::PxTunHelper => "ingress".to_string(),
        TunHelperKind::Tun2Socks => "socks5".to_string(),
    };
    let proxy_addr = match helper_kind {
        TunHelperKind::PxTunHelper => suggested_ingress_bind_addr(config),
        TunHelperKind::Tun2Socks => proxy.addr.clone(),
    };
    let helper_target_display = match helper_kind {
        TunHelperKind::PxTunHelper => format!("ingress://{proxy_addr}"),
        TunHelperKind::Tun2Socks => proxy.display.clone(),
    };
    let dns_helper_path = resolve_macos_dns_helper(runtime_dir)?;
    let helper_log_path = tun_helper_log_path(runtime_dir);
    let helper_pid_path = tun_helper_pid_path(runtime_dir);
    if let Some(parent) = helper_log_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let _ = fs::remove_file(&helper_log_path);
    let _ = fs::remove_file(&helper_pid_path);
    let script_path = resolve_macos_tun_helper_script(runtime_dir)?;
    append_log_line(
        logs,
        "tun",
        &format!(
            "macOS TUN 需要管理员权限，准备启动提权 helper。script={} helper={} dns_helper={}",
            script_path.display(),
            helper_path.display(),
            dns_helper_path.display()
        ),
    );

    let output = match run_macos_privileged_command(&[
        "/bin/bash".to_string(),
        script_path.display().to_string(),
        "start".to_string(),
        helper_path.display().to_string(),
        dns_helper_path.display().to_string(),
        proxy_mode,
        proxy_addr,
        config.tun.device_name.clone(),
        route_plan.primary_interface.clone(),
        config.tun.mtu.to_string(),
        map_tun_log_level(&config.log_level).to_string(),
        route_plan.tun_ipv4.clone(),
        route_plan.primary_gateway.clone(),
        route_plan.server_ip.clone(),
        helper_log_path.display().to_string(),
        helper_pid_path.display().to_string(),
    ]) {
        Ok(output) => output,
        Err(error) => {
            std::thread::sleep(Duration::from_millis(1200));
            if let Some(pid) = read_running_pid_from_file(&helper_pid_path)? {
                append_log_line(
                    logs,
                    "tun",
                    &format!(
                        "管理员权限脚本返回异常，但 helper 已成功启动，按成功处理。pid={} error={}",
                        pid, error
                    ),
                );
                return Ok(TunProcess::Privileged { pid });
            }
            return Err(error);
        }
    };
    let pid = output
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("提权 helper 返回了无效 pid: {output}"))?;

    std::thread::sleep(Duration::from_millis(800));
    if !is_process_running(pid) {
        append_tun_helper_log_tail(runtime_dir, logs);
        return Err("TUN helper 提权启动失败，请查看最近日志。".to_string());
    }

    append_log_line(
        logs,
        "tun",
        &format!(
            "已通过管理员权限启动 helper: {} -> {} ({})，日志: {}",
            helper_path.display(),
            helper_target_display,
            format_tun_summary(&config.tun),
            helper_log_path.display()
        ),
    );
    Ok(TunProcess::Privileged { pid })
}

fn helper_relative_path() -> &'static str {
    if cfg!(target_os = "windows") {
        "bin/px-tun-helper.exe"
    } else if cfg!(target_os = "macos") {
        "bin/px-tun-helper"
    } else {
        "bin/tun2socks"
    }
}

fn dns_helper_relative_path() -> &'static str {
    if cfg!(target_os = "windows") {
        "bin/px-dns-helper.exe"
    } else {
        "bin/px-dns-helper"
    }
}

fn run_tun_helper_repair_script(runtime_dir: &Path, logs: &Arc<Mutex<String>>) -> Result<RepairTunHelperResult, String> {
    let bin_dir = runtime_dir.join("bin");
    fs::create_dir_all(&bin_dir).map_err(|error| error.to_string())?;
    let helper_path = bin_dir.join(Path::new(helper_relative_path()).file_name().unwrap_or_default());
    let script_path = match resolve_repair_tun_helper_script(runtime_dir) {
        Ok(path) => path,
        Err(error) => {
            if cfg!(target_os = "macos") {
                return Err(format!(
                    "{error} macOS 正式发布包默认已自带 px-tun-helper；若当前缺失，请重新解压发布包。"
                ));
            }
            return Err(error);
        }
    };

    append_log_line(logs, "tun", &format!("开始修复 helper: {}", script_path.display()));
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
        if cfg!(target_os = "macos") {
            return Err("macOS helper 构建或补齐失败。开发环境请执行 scripts/install-dev-px-tun-helper.sh；正式发布包若缺失 helper，请重新解压发布包。".to_string());
        }
        return Err("修复 helper 失败，请检查脚本权限、Go 构建环境或下载源可达性。".to_string());
    }

    if !helper_path.exists() {
        return Err(format!("修复完成后仍未找到 helper: {}", helper_path.display()));
    }

    let wintun_path = if cfg!(target_os = "windows") {
        let path = bin_dir.join("wintun.dll");
        if !path.exists() {
            return Err(format!("修复完成后仍未找到 wintun.dll: {}", path.display()));
        }
        Some("bin/wintun.dll".to_string())
    } else {
        None
    };

    append_log_line(logs, "tun", &format!("helper 已就绪: {}", helper_path.display()));
    Ok(RepairTunHelperResult {
        helper_path: helper_relative_path().to_string(),
        wintun_path,
        message: "TUN helper 已修复到当前运行目录的 bin/。".to_string(),
    })
}

fn resolve_repair_tun_helper_script(runtime_dir: &Path) -> Result<PathBuf, String> {
    let script_name = if cfg!(target_os = "windows") {
        "repair-tun-helper.ps1"
    } else if cfg!(target_os = "macos") && cfg!(debug_assertions) {
        "install-dev-px-tun-helper.sh"
    } else {
        return Err(
            "当前平台不再提供独立 helper 获取脚本；macOS 开发态请使用 install-dev-px-tun-helper.sh，正式发布包缺失 helper 时请重新解压发布包。".to_string()
        );
    };
    resolve_runtime_script(runtime_dir, script_name).ok_or_else(|| {
        format!("未找到 {script_name}，请确认当前运行目录是发布目录，或在开发环境从 apps/tauri-ui 启动 GUI。")
    })
}

fn resolve_macos_tun_helper_script(runtime_dir: &Path) -> Result<PathBuf, String> {
    resolve_runtime_script(runtime_dir, "macos-tun-helper.sh").ok_or_else(|| {
        "未找到 macOS TUN 提权脚本，请确认当前运行目录包含 scripts/macos-tun-helper.sh。".to_string()
    })
}

fn resolve_runtime_script(runtime_dir: &Path, script_name: &str) -> Option<PathBuf> {
    for dir in runtime_dir.ancestors() {
        let candidate = dir.join("scripts").join(script_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_macos_dns_helper(runtime_dir: &Path) -> Result<PathBuf, String> {
    let packaged_path = runtime_dir.join(dns_helper_relative_path());
    if packaged_path.exists() {
        return Ok(packaged_path);
    }

    Err(format!(
        "未找到 macOS DNS helper: {}。请先构建或打包 px-dns-helper，再启动 TUN。",
        packaged_path.display()
    ))
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
        if guard.is_none() {
            if let Some(runtime) = recover_tun_runtime()? {
                *guard = Some(runtime);
            }
        }
        guard.take()
    };

    if let Some(runtime) = runtime {
        stop_tun_runtime(runtime, &state.logs)?;
    }
    Ok(())
}

fn stop_tun_runtime(runtime: TunRuntime, logs: &Arc<Mutex<String>>) -> Result<(), String> {
    match runtime.process {
        TunProcess::Local(mut child) => {
            cleanup_tun_routes(&runtime.route_plan, logs)?;
            let _ = child.kill();
            let _ = child.wait();
            Ok(())
        }
        TunProcess::Privileged { pid } => stop_macos_privileged_tun(&runtime.runtime_dir, &runtime.route_plan, pid),
    }
}

fn recover_tun_runtime() -> Result<Option<TunRuntime>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }

    let runtime_dir = runtime_dir().map_err(|error| error.to_string())?;
    let pid_path = tun_helper_pid_path(&runtime_dir);
    if !pid_path.exists() {
        return Ok(None);
    }

    let Some(pid) = read_running_pid_from_file(&pid_path)? else {
        let _ = fs::remove_file(&pid_path);
        return Ok(None);
    };

    let config_path = client_config_path().map_err(|error| error.to_string())?;
    if !config_path.exists() {
        return Ok(None);
    }
    let config = load_client_config(&config_path).map_err(|error| error.to_string())?;
    let route_plan = validate_tun_start(&runtime_dir, &config)?;

    Ok(Some(TunRuntime {
        process: TunProcess::Privileged { pid },
        route_plan,
        runtime_dir,
    }))
}

fn stop_macos_privileged_tun(runtime_dir: &Path, plan: &TunRoutePlan, pid: u32) -> Result<(), String> {
    let script_path = resolve_macos_tun_helper_script(runtime_dir)?;
    let pid_path = tun_helper_pid_path(runtime_dir);
    let _ = run_macos_privileged_command(&[
        "bash".to_string(),
        script_path.display().to_string(),
        "stop".to_string(),
        plan.device_name.clone(),
        plan.tun_ipv4.clone(),
        plan.primary_gateway.clone(),
        plan.server_ip.clone(),
        pid_path.display().to_string(),
        pid.to_string(),
    ])?;
    Ok(())
}

fn tun_process_pid(process: &TunProcess) -> u32 {
    match process {
        TunProcess::Local(child) => child.id(),
        TunProcess::Privileged { pid } => *pid,
    }
}

fn tun_process_is_running(process: &mut TunProcess) -> Result<bool, String> {
    match process {
        TunProcess::Local(child) => match child.try_wait() {
            Ok(None) => Ok(true),
            Ok(Some(_)) => Ok(false),
            Err(error) => Err(error.to_string()),
        },
        TunProcess::Privileged { pid } => Ok(is_process_running(*pid)),
    }
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

fn run_macos_privileged_command(args: &[String]) -> Result<String, String> {
    let mut command = Command::new("osascript");
    for line in [
        "on run argv",
        "set cmd to \"\"",
        "repeat with arg in argv",
        "set cmd to cmd & space & quoted form of arg",
        "end repeat",
        "return do shell script cmd with administrator privileges",
        "end run",
    ] {
        command.arg("-e").arg(line);
    }
    command.args(args);

    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if !stderr.is_empty() { stderr } else { stdout };
    if message.contains("User canceled") {
        return Err("已取消管理员授权，TUN 未启动。".to_string());
    }
    if message.is_empty() {
        return Err("管理员权限脚本执行失败。".to_string());
    }
    Err(format!("管理员权限脚本执行失败: {message}"))
}

fn is_process_running(pid: u32) -> bool {
    let output = match Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
    {
        Ok(output) => output,
        Err(_) => return false,
    };

    output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
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
            let mut start = buffer.len().saturating_sub(LOG_LIMIT);
            while start < buffer.len() && !buffer.is_char_boundary(start) {
                start += 1;
            }
            let trimmed = buffer[start..].to_string();
            *buffer = trimmed;
        }
    }
}

fn append_tun_helper_log_tail(runtime_dir: &Path, logs: &Arc<Mutex<String>>) {
    let helper_log_path = tun_helper_log_path(runtime_dir);
    let Ok(text) = fs::read_to_string(&helper_log_path) else {
        return;
    };
    let lines: Vec<&str> = text.lines().rev().take(8).collect();
    if lines.is_empty() {
        return;
    }
    for line in lines.into_iter().rev() {
        append_log_line(logs, "tun-helper", line);
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
            repair_tun_helper_command,
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
