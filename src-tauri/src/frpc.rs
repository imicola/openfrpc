use crate::http;
use crate::logs::{parse_frpc_line, LogLine};
use crate::settings::{get_settings, Settings};
use chrono::Local;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncRead;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// phase: stopped | starting | running | reconnecting | stopping
#[derive(Serialize, Clone, Default)]
pub struct StatusSnap {
    pub phase: String,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub started_epoch: Option<i64>,
    pub uptime_secs: u64,
    pub connected: bool,
    pub run_id: Option<String>,
    pub admin_port: Option<u16>,
    pub last_error: Option<String>,
    pub restart_attempts: u32,
    /// 配置条目的 user 前缀（R18）：运行时代理名 = "{user}.{name}"
    pub user_prefix: Option<String>,
    /// 已保存但尚未应用到运行中进程的修改（R15）
    pub pending_apply: bool,
}

impl StatusSnap {
    pub fn is_process_alive(&self) -> bool {
        self.phase != "stopped"
    }
}

#[derive(Serialize, Clone, Default, Debug)]
pub struct ProxyLive {
    pub name: String,
    pub ptype: String,
    pub status: String,
    pub err: String,
    pub local_addr: String,
    pub remote_addr: String,
    /// 采样失败后保留的旧数据（R17）：界面应显示「未知」而非无条件在线
    pub stale: bool,
    /// 本次观测时间 "HH:MM:SS"
    pub observed_at: Option<String>,
}

pub struct Managed {
    pub child: Child,
    pub stop_requested: bool,
    /// 进程代次：每次 spawn 递增，后台任务据此拒绝旧进程写回新状态（R07/R08/R31）
    pub generation: u64,
}

pub struct ManagerInner {
    pub managed: tokio::sync::Mutex<Option<Managed>>,
    /// 生命周期互斥锁：串行化 start/stop/restart，保证任一时刻至多一次 spawn 决策（R03）
    pub lifecycle: tokio::sync::Mutex<()>,
    pub status: Mutex<StatusSnap>,
    pub logs: Mutex<VecDeque<LogLine>>,
    pub seq: AtomicU64,
    pub live: Mutex<Vec<ProxyLive>>,
    pub exiting: AtomicBool,
    pub restarts: AtomicU32,
    /// 用户停止请求的代数：重连等待期也能感知「等待期间用户按了停止」
    pub stop_epoch: AtomicU64,
    /// 进程代次计数
    pub generation: AtomicU64,
    /// 最近一次成功保存配置的时间（epoch 秒）：与 started_epoch 比较得出 pending_apply（R15）
    pub last_saved_epoch: AtomicI64,
}

#[derive(Clone)]
pub struct FrpcManager(pub Arc<ManagerInner>);

impl FrpcManager {
    pub fn new() -> FrpcManager {
        FrpcManager(Arc::new(ManagerInner {
            managed: tokio::sync::Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
            status: Mutex::new(StatusSnap {
                phase: "stopped".into(),
                ..Default::default()
            }),
            logs: Mutex::new(VecDeque::new()),
            seq: AtomicU64::new(0),
            live: Mutex::new(Vec::new()),
            exiting: AtomicBool::new(false),
            restarts: AtomicU32::new(0),
            stop_epoch: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            last_saved_epoch: AtomicI64::new(0),
        }))
    }
}

/// 当前存活子进程的代次（None = 无子进程）
async fn active_generation(mgr: &FrpcManager) -> Option<u64> {
    mgr.0.managed.lock().await.as_ref().map(|m| m.generation)
}

/// 自动重启退避（R09）：1, 2, 4, 8, 16, 30…（attempts 从 1 起）
pub fn backoff_secs(attempts: u32) -> u64 {
    2u64
        .saturating_pow(attempts.saturating_sub(1))
        .min(30)
}

/// 向日志流注入一条 OpenFrpc 自身的通知（如观测不可用的原因）
fn note(app: &AppHandle, mgr: &FrpcManager, level: &str, msg: String) {
    let id = mgr.0.seq.fetch_add(1, Ordering::Relaxed);
    let line = LogLine {
        id,
        ts: Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        level: level.into(),
        src: "openfrpc".into(),
        msg,
    };
    {
        let cap = get_settings(app).log_buffer_lines.max(200);
        let mut logs = mgr.0.logs.lock().unwrap();
        logs.push_back(line.clone());
        while logs.len() > cap {
            logs.pop_front();
        }
    }
    let _ = app.emit("frpc:log", &line);
}

/// 修改状态快照并广播（事件 + 托盘）
pub fn patch_status(app: &AppHandle, mgr: &FrpcManager, f: impl FnOnce(&mut StatusSnap)) {
    let snap = {
        let mut s = mgr.0.status.lock().unwrap();
        f(&mut *s);
        if let Some(epoch) = s.started_epoch {
            s.uptime_secs = (Local::now().timestamp() - epoch).max(0) as u64;
        } else {
            s.uptime_secs = 0;
        }
        if s.phase == "stopped" {
            s.pid = None;
            s.admin_port = None;
            s.connected = false;
        }
        // R15：保存晚于本次启动 → 有未应用的修改
        let saved = mgr.0.last_saved_epoch.load(Ordering::Relaxed);
        s.pending_apply = saved > 0
            && s.phase != "stopped"
            && s.started_epoch.map(|st| saved > st).unwrap_or(false);
        s.clone()
    };
    let _ = app.emit("frpc:status", &snap);
    refresh_tray(app, &snap);
}

fn clear_live(app: &AppHandle, mgr: &FrpcManager) {
    *mgr.0.live.lock().unwrap() = Vec::new();
    let _ = app.emit("frpc:proxies", Vec::<ProxyLive>::new());
}

/// 在 127.0.0.1 上找一个空闲端口
fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("探测空闲端口失败: {e}"))?
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| e.to_string())
}

/// frpc 管理接口端点（R16）：地址/端口/认证均从配置解析
#[derive(Clone, Debug)]
struct AdminEndpoint {
    host: String,
    port: u16,
    auth: Option<(String, String)>,
}

/// 解析已有 webServer 配置。Ok(None) = 未配置（由调用方注入）；Err = 配置形态暂不支持观测
fn parse_admin_endpoint(doc: &toml_edit::DocumentMut) -> Result<Option<AdminEndpoint>, String> {
    let Some(item) = doc.get("webServer") else {
        return Ok(None);
    };
    let Some(t) = item.as_table_like() else {
        return Err("webServer 不是表，无法解析管理接口".into());
    };
    if t.get("tls").is_some() {
        return Err("webServer 配置了 TLS，OpenFrpc 暂不支持对其观测".into());
    }
    let Some(port) = t
        .get("port")
        .and_then(|v| v.as_integer())
        .and_then(|v| u16::try_from(v).ok())
    else {
        return Err("webServer 缺少有效 port，无法观测".into());
    };
    let addr = t.get("addr").and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
    let host = normalize_admin_host(addr);
    let user = t.get("user").and_then(|v| v.as_str()).map(String::from);
    let password = t
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let auth = user.map(|u| (u, password));
    Ok(Some(AdminEndpoint { host, port, auth }))
}

fn normalize_admin_host(addr: &str) -> String {
    let a = addr.trim();
    if a.is_empty() || a == "0.0.0.0" || a == "::" || a == "[::]" {
        "127.0.0.1".into()
    } else if a == "::1" || a == "[::1]" {
        "::1".into()
    } else {
        a.to_string()
    }
}

/// 启动 frpc：与停止/重启串行（R03），已在运行则幂等返回
pub async fn start_frpc(app: AppHandle, mgr: FrpcManager, settings: Settings) -> Result<(), String> {
    if mgr.0.exiting.load(Ordering::Relaxed) {
        return Err("应用正在退出".into());
    }
    let _lg = mgr.0.lifecycle.lock().await;
    {
        let g = mgr.0.managed.lock().await;
        if g.is_some() {
            return Ok(());
        }
    }
    spawn_child(&app, &mgr, &settings).await
}

/// 只负责拉起子进程及其读日志/轮询任务（不递归派生 monitor）。
/// 调用方必须已持有 lifecycle 锁
async fn spawn_child(app: &AppHandle, mgr: &FrpcManager, settings: &Settings) -> Result<(), String> {
    let frpc_path = settings
        .frpc_path
        .clone()
        .filter(|p| std::path::Path::new(p).is_file())
        .ok_or("未找到 frpc.exe，请先在「设置」中选择路径")?;
    let cfg_path = settings
        .config_path
        .clone()
        .filter(|p| std::path::Path::new(p).is_file())
        .ok_or("未找到 frpc.toml，请先在「设置」中选择配置文件")?;

    let text = std::fs::read_to_string(&cfg_path).map_err(|e| format!("读取配置失败: {e}"))?;
    let doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("配置不是有效的 TOML: {e}"))?;

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?;
    let _ = std::fs::create_dir_all(&data_dir);

    // R18：记录 user 前缀，运行时代理名 = "{user}.{name}"
    let user_prefix = doc
        .get("user")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    // 管理接口端点：未配置则注入随机回环端口（R16：已有配置按其 addr/认证解析）
    let has_webserver = doc.get("webServer").is_some();
    let endpoint = match parse_admin_endpoint(&doc) {
        Ok(Some(ep)) => Some(ep),
        Ok(None) => Some(AdminEndpoint {
            host: "127.0.0.1".into(),
            port: free_port()?,
            auth: None,
        }),
        Err(e) => {
            note(app, mgr, "W", format!("管理接口观测不可用：{e}"));
            None
        }
    };

    // 实测 frpc 0.71 的多 -c 聚合在部分 frps 下握手异常，
    // 因此生成「用户配置 + webServer」的单个运行时副本，不碰原文件
    let mut runtime_text = text.clone();
    if !has_webserver {
        if let Some(ep) = &endpoint {
            let mut merged = String::with_capacity(text.len() + 64);
            merged.push_str(text.trim_end());
            merged.push_str("\n\n[webServer]\naddr = \"127.0.0.1\"\nport = ");
            merged.push_str(&ep.port.to_string());
            merged.push('\n');
            runtime_text = merged;
        }
    }
    let runtime_path = data_dir.join("frpdeck-runtime.toml");
    std::fs::write(&runtime_path, runtime_text).map_err(|e| format!("写入运行时配置失败: {e}"))?;

    let mut cmd = Command::new(&frpc_path);
    cmd.arg("-c").arg(&runtime_path);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // R03：即使句柄被意外覆盖/丢弃也杀掉子进程，杜绝失管实例
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("启动 frpc 失败: {e}"))?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (tx, rx) = mpsc::unbounded_channel::<String>();
    if let Some(out) = stdout {
        let boxed: Box<dyn AsyncRead + Send + Unpin> = Box::new(out);
        tauri::async_runtime::spawn(read_pipe(boxed, tx.clone()));
    }
    if let Some(err) = stderr {
        let boxed: Box<dyn AsyncRead + Send + Unpin> = Box::new(err);
        tauri::async_runtime::spawn(read_pipe(boxed, tx.clone()));
    }
    drop(tx);

    let gen = mgr.0.generation.fetch_add(1, Ordering::Relaxed) + 1;
    *mgr.0.managed.lock().await = Some(Managed {
        child,
        stop_requested: false,
        generation: gen,
    });

    let started_epoch = Local::now().timestamp();
    patch_status(app, mgr, |s| {
        s.phase = "starting".into();
        s.pid = pid;
        s.started_at = Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        s.started_epoch = Some(started_epoch);
        s.uptime_secs = 0;
        s.connected = false;
        s.run_id = None;
        s.last_error = None;
        s.admin_port = endpoint.as_ref().map(|e| e.port);
        s.user_prefix = user_prefix;
    });

    let app2 = app.clone();
    let mgr2 = mgr.clone();
    tauri::async_runtime::spawn(async move {
        pump_logs(app2, mgr2, rx, gen).await;
    });

    if let Some(ep) = endpoint {
        let app4 = app.clone();
        let mgr4 = mgr.clone();
        tauri::async_runtime::spawn(async move {
            poll_admin(app4, mgr4, ep, gen).await;
        });
    }

    // 连续运行满 60 秒后清零重启计数（R08：只清本代次进程的计数）
    let mgr5 = mgr.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        if active_generation(&mgr5).await == Some(gen) {
            mgr5.0.restarts.store(0, Ordering::Relaxed);
        }
    });

    Ok(())
}

/// 守护任务：应用启动后常驻，唯一拥有「意外退出 → 自动重启」决策权
pub async fn supervise(app: AppHandle, mgr: FrpcManager) {
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let step = {
            let mut g = mgr.0.managed.lock().await;
            match g.as_mut() {
                None => Step::Empty,
                Some(m) => match m.child.try_wait() {
                    Ok(Some(status)) => {
                        let stop_requested = m.stop_requested;
                        let _ = g.take();
                        Step::Exit {
                            stop_requested,
                            code: status.code(),
                        }
                    }
                    Ok(None) => Step::Alive,
                    Err(_) => {
                        let stop_requested = m.stop_requested;
                        let _ = g.take();
                        Step::Exit {
                            stop_requested,
                            code: None,
                        }
                    }
                },
            }
        };
        match step {
            Step::Alive | Step::Empty => continue, // Empty：尚无子进程，继续值守
            Step::Exit { stop_requested, code } => {
                patch_status(&app, &mgr, |s| {
                    s.phase = "stopped".into();
                    s.connected = false;
                });
                clear_live(&app, &mgr);
                // 仅应用退出时结束守护；用户停止后继续值守下一次启动
                if mgr.0.exiting.load(Ordering::Relaxed) {
                    return;
                }
                if stop_requested {
                    continue;
                }
                let settings = get_settings(&app);
                let code_desc = code
                    .map(|c| format!("退出码 {c}"))
                    .unwrap_or_else(|| "进程被终止".into());
                let attempts = mgr.0.restarts.fetch_add(1, Ordering::Relaxed) + 1;
                if settings.auto_restart && attempts <= 6 {
                    let secs = backoff_secs(attempts);
                    let epoch_before = mgr.0.stop_epoch.load(Ordering::Relaxed);
                    patch_status(&app, &mgr, |s| {
                        s.phase = "reconnecting".into();
                        s.last_error =
                            Some(format!("frpc 意外退出（{code_desc}），{secs} 秒后自动重试"));
                        s.restart_attempts = attempts;
                    });
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                    // 等待期间用户按了停止：放弃重启，落回已停止
                    if mgr.0.exiting.load(Ordering::Relaxed) {
                        return;
                    }
                    if mgr.0.stop_epoch.load(Ordering::Relaxed) != epoch_before {
                        patch_status(&app, &mgr, |s| {
                            s.phase = "stopped".into();
                        });
                        continue;
                    }
                    // 等待期间可能已被再次手动启动，交回轮询
                    if mgr.0.managed.lock().await.is_some() {
                        continue;
                    }
                    let settings = get_settings(&app);
                    // R03：自动重启同样经由生命周期锁，与手动启停互斥
                    let _lg = mgr.0.lifecycle.lock().await;
                    if mgr.0.managed.lock().await.is_some() {
                        continue;
                    }
                    match spawn_child(&app, &mgr, &settings).await {
                        Ok(()) => continue, // 新子进程仍由本任务守护
                        Err(e) => {
                            // R26B：重启失败进入明确终态，不永远停留在「重连中」
                            mgr.0.restarts.store(0, Ordering::Relaxed);
                            patch_status(&app, &mgr, |s| {
                                s.phase = "stopped".into();
                                s.last_error = Some(format!(
                                    "自动重启失败: {e}；已停止自动重试，可手动启动"
                                ));
                                s.restart_attempts = 0;
                            });
                            continue;
                        }
                    }
                } else {
                    patch_status(&app, &mgr, |s| {
                        s.last_error = Some(format!("frpc 已停止（意外退出，{code_desc}）"));
                        s.restart_attempts = 0;
                    });
                    mgr.0.restarts.store(0, Ordering::Relaxed);
                    continue;
                }
            }
        }
    }
}

/// 停止实现（调用方需持有 lifecycle 锁）。
/// R26A：等待进程真正退出才报成功，超时/kill 失败如实上报
async fn stop_internal(app: &AppHandle, mgr: &FrpcManager) -> Result<String, String> {
    let mut g = mgr.0.managed.lock().await;
    match g.as_mut() {
        Some(m) => {
            m.stop_requested = true;
            let pid = m.child.id();
            let kill_err = m.child.start_kill().err().map(|e| e.to_string());
            drop(g);
            patch_status(app, mgr, |s| {
                s.phase = "stopping".into();
                s.connected = false;
            });
            for _ in 0..50 {
                if mgr.0.managed.lock().await.is_none() {
                    patch_status(app, mgr, |s| {
                        s.phase = "stopped".into();
                        s.connected = false;
                    });
                    clear_live(app, mgr);
                    return Ok(format!("frpc 已停止（PID {}）", pid.unwrap_or(0)));
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(format!(
                "停止超时：frpc（PID {}）未在 10 秒内退出{}；可到「设置 → 运行中的 frpc 进程」强制结束",
                pid.unwrap_or(0),
                kill_err.map(|e| format!("（终止调用失败：{e}）")).unwrap_or_default()
            ))
        }
        None => {
            drop(g);
            // 没有存活子进程：可能是重连等待中被停止，直接落回已停止态
            patch_status(app, mgr, |s| {
                s.phase = "stopped".into();
                s.connected = false;
            });
            clear_live(app, mgr);
            Ok("frpc 未在运行".into())
        }
    }
}

pub async fn stop_frpc(app: AppHandle, mgr: FrpcManager) -> Result<String, String> {
    // 无论当前是否有存活子进程，都记录一次用户停止意图（供重连等待期判断）
    mgr.0.stop_epoch.fetch_add(1, Ordering::Relaxed);
    let _lg = mgr.0.lifecycle.lock().await;
    stop_internal(&app, &mgr).await
}

/// 重启：停止 → 等待回收 → 启动 → 确认新代次（R26：不再假报成功）
pub async fn restart_frpc(app: AppHandle, mgr: FrpcManager) -> Result<String, String> {
    let _lg = mgr.0.lifecycle.lock().await;
    let was_running = mgr.0.managed.lock().await.is_some();
    if was_running {
        stop_internal(&app, &mgr).await?;
    }
    let settings = get_settings(&app);
    spawn_child(&app, &mgr, &settings).await?;
    let pid = mgr
        .0
        .managed
        .lock()
        .await
        .as_ref()
        .and_then(|m| m.child.id());
    patch_status(&app, &mgr, |s| {
        s.last_error = None;
    });
    Ok(format!("frpc 已重启（新 PID {}）", pid.unwrap_or(0)))
}

async fn read_pipe(mut pipe: Box<dyn AsyncRead + Unpin + Send>, tx: mpsc::UnboundedSender<String>) {
    use tokio::io::AsyncReadExt;
    let mut chunk = [0u8; 4096];
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let end = if pos > 0 && buf[pos - 1] == b'\r' { pos - 1 } else { pos };
                    let line = String::from_utf8_lossy(&buf[..end]).to_string();
                    buf.drain(..=pos);
                    if !line.trim().is_empty() {
                        let _ = tx.send(line);
                    }
                }
            }
        }
    }
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf).to_string();
        if !line.trim().is_empty() {
            let _ = tx.send(line);
        }
    }
}

async fn pump_logs(app: AppHandle, mgr: FrpcManager, mut rx: mpsc::UnboundedReceiver<String>, gen: u64) {
    while let Some(raw) = rx.recv().await {
        let p = parse_frpc_line(&raw);
        let id = mgr.0.seq.fetch_add(1, Ordering::Relaxed);
        let ts = if p.ts.is_empty() {
            Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
        } else {
            p.ts.clone()
        };
        let line = LogLine {
            id,
            ts,
            level: p.level.clone(),
            src: p.src.clone(),
            msg: p.msg.clone(),
        };
        {
            let cap = get_settings(&app).log_buffer_lines.max(200);
            let mut logs = mgr.0.logs.lock().unwrap();
            logs.push_back(line.clone());
            while logs.len() > cap {
                logs.pop_front();
            }
        }
        let _ = app.emit("frpc:log", &line);

        // R07/R31：状态写入先校验代次；只有存活相位能被日志升级为 running，
        // stopped/stopping 不得被迟到的成功日志翻回
        let msg = p.msg.as_str();
        if msg.contains("login to server success") {
            if active_generation(&mgr).await == Some(gen) {
                let rid = p.run_id.clone().or_else(|| extract_run_id(msg));
                patch_status(&app, &mgr, |s| {
                    if matches!(s.phase.as_str(), "starting" | "reconnecting" | "running") {
                        s.phase = "running".into();
                        s.connected = true;
                        s.run_id = rid.clone();
                        s.last_error = None;
                    }
                });
            }
        } else if msg.contains("connect to server error")
            || msg.contains("login to the server failed")
            || msg.contains("connect to server failed")
        {
            if active_generation(&mgr).await == Some(gen) {
                let err: String = msg.chars().take(220).collect();
                patch_status(&app, &mgr, |s| {
                    s.connected = false;
                    s.last_error = Some(err.clone());
                    if s.is_process_alive() {
                        s.phase = "reconnecting".into();
                    }
                });
            }
        }
    }
}

fn extract_run_id(msg: &str) -> Option<String> {
    let start = msg.rfind('[')?;
    let end = msg.rfind(']')?;
    if end > start && end - start == 17 {
        Some(msg[start + 1..end].to_string())
    } else {
        None
    }
}

enum Step {
    Alive,
    Exit { stop_requested: bool, code: Option<i32> },
    Empty,
}

/// 轮询 frpc 管理接口 /api/status，获得每条代理的实时状态。
/// R07：绑定进程代次，重启后旧任务自动退出；响应写回前再次校验
async fn poll_admin(app: AppHandle, mgr: FrpcManager, ep: AdminEndpoint, gen: u64) {
    let mut empty_polls = 0u32;
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        if active_generation(&mgr).await != Some(gen) {
            return;
        }
        let auth = ep
            .auth
            .as_ref()
            .map(|(u, p)| (u.as_str(), p.as_str()));
        let res =
            http::get_json(&ep.host, ep.port, "/api/status", auth, Duration::from_secs(2)).await;
        match res {
            Ok(v) => {
                if active_generation(&mgr).await != Some(gen) {
                    return;
                }
                let now = Local::now().format("%H:%M:%S").to_string();
                let list: Vec<ProxyLive> = normalize_status(&v)
                    .into_iter()
                    .map(|mut p| {
                        p.observed_at = Some(now.clone());
                        p
                    })
                    .collect();
                let has_running = list.iter().any(|p| p.status == "running");
                {
                    let mut live = mgr.0.live.lock().unwrap();
                    *live = list.clone();
                }
                let _ = app.emit("frpc:proxies", &list);
                if has_running {
                    empty_polls = 0;
                    patch_status(&app, &mgr, |s| {
                        if s.phase == "starting" || s.phase == "reconnecting" {
                            s.phase = "running".into();
                            s.connected = true;
                        }
                    });
                } else {
                    empty_polls += 1;
                    // R31：日志写到文件/级别不足时「登录成功」文本信号可能缺失；
                    // 管理接口连续可达说明进程健在，从「启动中」收敛为
                    // 「运行中 · 尚未确认登录」，不再永远卡在启动中
                    if empty_polls >= 3 {
                        patch_status(&app, &mgr, |s| {
                            if s.phase == "starting" {
                                s.phase = "running".into();
                            }
                        });
                    }
                }
            }
            Err(_) => {
                // R17：采样失败不清空旧数据，标记可能过期，界面显示「未知」
                if active_generation(&mgr).await != Some(gen) {
                    return;
                }
                let snapshot = {
                    let mut live = mgr.0.live.lock().unwrap();
                    if live.is_empty() {
                        None
                    } else {
                        for p in live.iter_mut() {
                            p.stale = true;
                        }
                        Some(live.clone())
                    }
                };
                if let Some(list) = snapshot {
                    let _ = app.emit("frpc:proxies", &list);
                }
            }
        }
    }
}

fn normalize_status(v: &serde_json::Value) -> Vec<ProxyLive> {
    let mut out = Vec::new();
    for key in ["tcp", "udp", "http", "https", "stcp", "sudp", "xtcp"] {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            for item in arr {
                let g = |field: &str| {
                    item.get(field)
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                out.push(ProxyLive {
                    name: g("name"),
                    ptype: key.to_string(),
                    status: g("status"),
                    err: g("err"),
                    local_addr: g("local_addr"),
                    remote_addr: g("remote_addr"),
                    stale: false,
                    observed_at: None,
                });
            }
        }
    }
    out
}

// ---------- 系统托盘 ----------

fn tray_menu(app: &AppHandle, running: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        if running { "停止 frpc" } else { "启动 frpc" },
        true,
        None::<&str>,
    )?;
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 OpenFrpc", true, None::<&str>)?;
    Menu::with_items(app, &[&toggle, &show, &quit])
}

fn phase_zh(phase: &str) -> &'static str {
    match phase {
        "starting" => "启动中",
        "running" => "运行中",
        "reconnecting" => "重连中",
        "stopping" => "停止中",
        _ => "已停止",
    }
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let running = app
        .state::<FrpcManager>()
        .0
        .status
        .lock()
        .unwrap()
        .is_process_alive();
    let menu = tray_menu(app, running)?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("缺少窗口图标").clone())
        .tooltip("OpenFrpc")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let mgr = app.state::<FrpcManager>().inner().clone();
                    let running = mgr.0.status.lock().unwrap().is_process_alive();
                    if running {
                        if let Err(e) = stop_frpc(app.clone(), mgr.clone()).await {
                            patch_status(&app, &mgr, |s| {
                                s.last_error = Some(e.clone());
                            });
                        }
                    } else {
                        let settings = get_settings(&app);
                        if let Err(e) = start_frpc(app.clone(), mgr.clone(), settings).await {
                            patch_status(&app, &mgr, |s| {
                                s.last_error = Some(e.clone());
                            });
                        }
                    }
                });
            }
            "show" => show_main(app),
            "quit" => {
                app.state::<FrpcManager>()
                    .0
                    .exiting
                    .store(true, Ordering::Relaxed);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn refresh_tray(app: &AppHandle, snap: &StatusSnap) {
    if let Some(tray) = app.tray_by_id("main") {
        let running = snap.is_process_alive();
        if let Ok(menu) = tray_menu(app, running) {
            let _ = tray.set_menu(Some(menu));
        }
        let conn = if running && snap.connected {
            " · 已连接服务器"
        } else {
            ""
        };
        let _ = tray.set_tooltip(Some(format!("OpenFrpc — {}{}", phase_zh(&snap.phase), conn)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_exponential_capped() {
        let seq: Vec<u64> = (1..=6).map(backoff_secs).collect();
        assert_eq!(seq, vec![1, 2, 4, 8, 16, 30]);
        assert_eq!(backoff_secs(10), 30);
        assert_eq!(backoff_secs(0), 1);
    }

    #[test]
    fn admin_host_normalization() {
        assert_eq!(normalize_admin_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(normalize_admin_host(""), "127.0.0.1");
        assert_eq!(normalize_admin_host("::"), "127.0.0.1");
        assert_eq!(normalize_admin_host("[::1]"), "::1");
        assert_eq!(normalize_admin_host("192.168.1.2"), "192.168.1.2");
    }

    #[test]
    fn admin_endpoint_parses_inline_and_auth() {
        let doc = "webServer = { addr = \"0.0.0.0\", port = 7400, user = \"u\", password = \"p\" }\n"
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let ep = parse_admin_endpoint(&doc).unwrap().unwrap();
        assert_eq!(ep.host, "127.0.0.1");
        assert_eq!(ep.port, 7400);
        assert_eq!(ep.auth.as_ref().unwrap().0, "u");

        let doc = "webServer = { addr = \"127.0.0.1\" }\n"
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert!(parse_admin_endpoint(&doc).is_err());

        let doc = "serverAddr = \"x\"\n"
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert!(parse_admin_endpoint(&doc).unwrap().is_none());
    }

    #[test]
    fn user_prefix_extracted_from_doc() {
        let doc = "user = \"alice\"\nserverAddr = \"x\"\n"
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let u = doc
            .get("user")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        assert_eq!(u.as_deref(), Some("alice"));
    }
}
