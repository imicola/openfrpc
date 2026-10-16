use crate::config::{self, ConfigInfo, ProxyEntry, SaveResult, ServerFields};
use crate::frpc::{self, FrpcManager, ProxyLive, StatusSnap};
use crate::listeners;
use crate::scan::{self, PortResult, ProbeResult};
use crate::settings::{get_settings, Settings, SettingsState};
use serde::Serialize;
use std::fs;
use std::path::Path;
use tauri::{AppHandle, Manager, State};

#[derive(Serialize)]
pub struct DetectInfo {
    pub path: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct InitInfo {
    pub settings: Settings,
    pub settings_warning: Option<String>,
    pub detected: Option<DetectInfo>,
    pub app_version: String,
    pub data_dir: String,
    pub config: Option<ConfigInfo>,
    pub status: StatusSnap,
    pub logs: Vec<crate::logs::LogLine>,
    pub live: Vec<ProxyLive>,
    pub frpc_processes: Vec<listeners::FrpcProcess>,
}

fn frpc_version_of(path: &str) -> String {
    listeners::run_hidden_capture(path, &["-v"], 5)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// 探测 frpc.exe：配置旁 → 记忆路径 → D:\apps\frp 递归 → PATH
pub fn detect_frpc_path(settings: &Settings) -> Option<DetectInfo> {
    // 1. 配置文件同目录
    if let Some(cfg) = &settings.config_path {
        if let Some(dir) = Path::new(cfg).parent() {
            let cand = dir.join("frpc.exe");
            if cand.is_file() {
                let p = cand.to_string_lossy().to_string();
                return Some(DetectInfo { version: frpc_version_of(&p), path: p });
            }
        }
    }
    // 2. 记忆的路径
    if let Some(p) = &settings.frpc_path {
        if Path::new(p).is_file() {
            return Some(DetectInfo { version: frpc_version_of(p), path: p.clone() });
        }
    }
    // 3. 常见安装根目录（限深 3 层）
    for root in ["D:\\apps\\frp", "C:\\frp", "D:\\frp"] {
        if let Some(found) = find_under(Path::new(root), 3) {
            return Some(DetectInfo { version: frpc_version_of(&found), path: found });
        }
    }
    // 4. PATH
    if let Some(o) = listeners::run_hidden_capture("where", &["frpc"], 5) {
        if o.status.success() {
            let first = String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !first.is_empty() && Path::new(&first).is_file() {
                return Some(DetectInfo { version: frpc_version_of(&first), path: first });
            }
        }
    }
    None
}

fn find_under(root: &Path, depth: u32) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let direct = root.join("frpc.exe");
    if direct.is_file() {
        return Some(direct.to_string_lossy().to_string());
    }
    let rd = fs::read_dir(root).ok()?;
    for entry in rd.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_under(&path, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// 记录一次成功保存（epoch 秒）并立即广播状态（派生 pending_apply），供 UI 提示「待应用」（R15）
fn commit_save(app: &AppHandle, mgr: &FrpcManager, res: &mut SaveResult) {
    mgr.0
        .last_saved_epoch
        .store(chrono::Local::now().timestamp(), std::sync::atomic::Ordering::Relaxed);
    // 填充 applied：frpc 未运行 = 下次启动即生效；运行中则比较启动与保存时间
    let snap = mgr.0.status.lock().unwrap();
    if snap.phase == "stopped" {
        res.applied = true;
    } else {
        let saved = mgr.0.last_saved_epoch.load(std::sync::atomic::Ordering::Relaxed);
        res.applied = snap.started_epoch.map(|st| saved <= st).unwrap_or(true);
    }
    drop(snap);
    frpc::patch_status(app, mgr, |_| {});
}

#[tauri::command]
pub async fn app_init(app: AppHandle, mgr: State<'_, FrpcManager>) -> Result<InitInfo, String> {
    // R10：探测/版本查询涉及子进程与磁盘扫描，放到阻塞线程池，不卡主线程
    let app = app.clone();
    let mgr_c = mgr.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<SettingsState>();
        let mut settings = state.get();
        let load_warning = state.load_warning.clone();
        let detected = detect_frpc_path(&settings);
        // 首次运行：定位到 frpc 后自动记忆路径，并关联同目录 frpc.toml
        if settings.frpc_path.is_none() || settings.config_path.is_none() {
            if let Some(det) = &detected {
                if settings.frpc_path.is_none() {
                    settings.frpc_path = Some(det.path.clone());
                }
                if settings.config_path.is_none() {
                    let cand = std::path::Path::new(&det.path).with_file_name("frpc.toml");
                    if cand.is_file() {
                        settings.config_path = Some(cand.to_string_lossy().to_string());
                    }
                }
                *state.inner.write().unwrap() = settings.clone();
                let _ = settings.save(&state.path);
            }
        }
        let config = config::read_config(&app).ok();
        let mut status = mgr_c.0.status.lock().unwrap().clone();
        if let Some(epoch) = status.started_epoch {
            status.uptime_secs = (chrono::Local::now().timestamp() - epoch).max(0) as u64;
        }
        let logs = mgr_c.0.logs.lock().unwrap().iter().cloned().collect();
        let live = mgr_c.0.live.lock().unwrap().clone();
        Ok(InitInfo {
            settings,
            settings_warning: load_warning,
            detected,
            app_version: app.package_info().version.to_string(),
            data_dir: app
                .path()
                .app_data_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            config,
            status,
            logs,
            live,
            frpc_processes: listeners::list_frpc_processes(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn detect_frpc(app: AppHandle) -> Result<Option<DetectInfo>, String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || detect_frpc_path(&get_settings(&app)))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_save(_app: AppHandle, state: State<'_, SettingsState>, settings: Settings) -> Result<(), String> {
    // R22：先持久化成功，再提交内存状态；失败时两端保持旧值
    settings.save(&state.path)?;
    *state.inner.write().unwrap() = settings;
    Ok(())
}

#[tauri::command]
pub async fn frpc_start(app: AppHandle, mgr: State<'_, FrpcManager>) -> Result<(), String> {
    let settings = get_settings(&app);
    frpc::start_frpc(app.clone(), mgr.inner().clone(), settings).await
}

#[tauri::command]
pub async fn frpc_stop(app: AppHandle, mgr: State<'_, FrpcManager>) -> Result<String, String> {
    frpc::stop_frpc(app.clone(), mgr.inner().clone()).await
}

#[tauri::command]
pub async fn frpc_restart(app: AppHandle, mgr: State<'_, FrpcManager>) -> Result<String, String> {
    frpc::restart_frpc(app.clone(), mgr.inner().clone()).await
}

#[tauri::command]
pub fn frpc_status(mgr: State<'_, FrpcManager>) -> StatusSnap {
    let mut snap = mgr.0.status.lock().unwrap().clone();
    if let Some(epoch) = snap.started_epoch {
        snap.uptime_secs = (chrono::Local::now().timestamp() - epoch).max(0) as u64;
    }
    // 与 patch_status 相同的派生逻辑，保证命令查询一致
    let saved = mgr.0.last_saved_epoch.load(std::sync::atomic::Ordering::Relaxed);
    snap.pending_apply = saved > 0
        && snap.phase != "stopped"
        && snap.started_epoch.map(|st| saved > st).unwrap_or(false);
    snap
}

#[tauri::command]
pub fn frpc_processes() -> Vec<listeners::FrpcProcess> {
    listeners::list_frpc_processes()
}

/// R25：结束一个 frpc 进程。
/// - 本应用管理的实例 → 走正常停止流程（stop_requested，不触发自动重启）；
/// - 外部实例 → 先复核该 PID 仍是 frpc.exe（防 PID 复用误杀），再强杀
#[tauri::command]
pub async fn frpc_kill_pid(app: AppHandle, mgr: State<'_, FrpcManager>, pid: u32) -> Result<String, String> {
    {
        let g = mgr.0.managed.lock().await;
        if let Some(m) = g.as_ref() {
            if m.child.id() == Some(pid) {
                drop(g);
                return frpc::stop_frpc(app, mgr.inner().clone()).await;
            }
        }
    }
    tauri::async_runtime::spawn_blocking(move || listeners::verify_and_kill_frpc(pid))
        .await
        .map_err(|e| e.to_string())?
        .map(|()| format!("已结束外部 frpc 进程（PID {pid}）"))
}

#[tauri::command]
pub fn logs_snapshot(mgr: State<'_, FrpcManager>) -> Vec<crate::logs::LogLine> {
    mgr.0.logs.lock().unwrap().iter().cloned().collect()
}

#[tauri::command]
pub fn live_proxies(mgr: State<'_, FrpcManager>) -> Vec<ProxyLive> {
    mgr.0.live.lock().unwrap().clone()
}

#[tauri::command]
pub fn config_read(app: AppHandle) -> Result<ConfigInfo, String> {
    config::read_config(&app)
}

#[tauri::command]
pub async fn config_validate(app: AppHandle, text: String) -> Result<ValidateResult, String> {
    // R10：frpc verify 可能因包装程序/管道持有而长时间不返回，放入阻塞线程并受真实超时保护
    let res = tauri::async_runtime::spawn_blocking(move || config::validate_only(&app, &text))
        .await
        .map_err(|e| e.to_string())?;
    let (ok, output) = res?;
    Ok(ValidateResult { ok, output })
}

#[derive(Serialize)]
pub struct ValidateResult {
    pub ok: bool,
    pub output: String,
}

#[tauri::command]
pub fn config_backups(app: AppHandle) -> Vec<config::BackupInfo> {
    config::list_backups(&app)
}

#[tauri::command]
pub async fn config_restore(app: AppHandle, mgr: State<'_, FrpcManager>, path: String) -> Result<SaveResult, String> {
    let app = app.clone();
    let mgr_c = mgr.inner().clone();
    let app2 = app.clone();
    let mut res = tauri::async_runtime::spawn_blocking(move || config::restore_backup(&app, &path))
        .await
        .map_err(|e| e.to_string())??;
    commit_save(&app2, &mgr_c, &mut res);
    Ok(res)
}

#[tauri::command]
pub async fn config_save_text(
    app: AppHandle,
    mgr: State<'_, FrpcManager>,
    text: String,
    validate: bool,
    expected_rev: Option<String>,
) -> Result<SaveResult, String> {
    let app = app.clone();
    let mgr_c = mgr.inner().clone();
    let app2 = app.clone();
    let mut res = tauri::async_runtime::spawn_blocking(move || {
        config::save_text(&app, &text, validate, expected_rev.as_deref())
    })
    .await
    .map_err(|e| e.to_string())??;
    commit_save(&app2, &mgr_c, &mut res);
    Ok(res)
}

#[tauri::command]
pub async fn config_save_server(app: AppHandle, mgr: State<'_, FrpcManager>, fields: ServerFields) -> Result<SaveResult, String> {
    let app = app.clone();
    let mgr_c = mgr.inner().clone();
    let app2 = app.clone();
    let mut res = tauri::async_runtime::spawn_blocking(move || {
        let info = config::read_config(&app)?;
        let new_text = config::apply_server_fields(&info.text, &fields)?;
        config::save_text(&app, &new_text, true, None)
    })
    .await
    .map_err(|e| e.to_string())??;
    commit_save(&app2, &mgr_c, &mut res);
    Ok(res)
}

#[tauri::command]
pub async fn proxy_upsert(app: AppHandle, mgr: State<'_, FrpcManager>, old_name: Option<String>, entry: ProxyEntry) -> Result<SaveResult, String> {
    let app = app.clone();
    let mgr_c = mgr.inner().clone();
    let app2 = app.clone();
    let mut res = tauri::async_runtime::spawn_blocking(move || {
        let info = config::read_config(&app)?;
        let new_text = config::upsert_proxy(&info.text, old_name.as_deref(), &entry)?;
        config::save_text(&app, &new_text, true, None)
    })
    .await
    .map_err(|e| e.to_string())??;
    commit_save(&app2, &mgr_c, &mut res);
    Ok(res)
}

#[tauri::command]
pub async fn proxy_delete(app: AppHandle, mgr: State<'_, FrpcManager>, name: String) -> Result<SaveResult, String> {
    let app = app.clone();
    let mgr_c = mgr.inner().clone();
    let app2 = app.clone();
    let mut res = tauri::async_runtime::spawn_blocking(move || {
        let info = config::read_config(&app)?;
        let new_text = config::delete_proxy(&info.text, &name)?;
        config::save_text(&app, &new_text, false, None)
    })
    .await
    .map_err(|e| e.to_string())??;
    commit_save(&app2, &mgr_c, &mut res);
    Ok(res)
}

#[tauri::command]
pub async fn scan_ports(
    app: AppHandle,
    host: String,
    ports: Vec<u16>,
    timeout_ms: u64,
    concurrency: u64,
    task: Option<String>,
) -> Result<Vec<PortResult>, String> {
    scan::scan_ports(&app, &host, ports, timeout_ms, concurrency as usize, task).await
}

#[tauri::command]
pub fn listeners_list() -> Result<Vec<listeners::Listener>, String> {
    listeners::list_listeners()
}

#[tauri::command]
pub async fn probe_server(host: String, port: u16, count: u32) -> Result<ProbeResult, String> {
    scan::probe_server(&host, port, count).await
}

#[tauri::command]
pub fn export_logs(app: AppHandle, mgr: State<'_, FrpcManager>, path: String) -> Result<usize, String> {
    let logs = mgr.0.logs.lock().unwrap();
    let mut text = String::with_capacity(logs.len() * 128);
    for l in logs.iter() {
        text.push_str(&format!("[{}] [{}] [{}] {}\n", l.ts, l.level, l.src, l.msg));
    }
    let n = logs.len();
    drop(logs);
    fs::write(&path, text).map_err(|e| format!("写出日志失败: {e}"))?;
    let _ = app;
    Ok(n)
}

#[tauri::command]
pub fn logs_clear(mgr: State<'_, FrpcManager>) {
    mgr.0.logs.lock().unwrap().clear();
}
