mod commands;
mod config;
mod frpc;
mod http;
mod listeners;
mod logs;
mod scan;
mod settings;

use frpc::FrpcManager;
use settings::{get_settings, SettingsState};
use std::sync::RwLock;
use std::time::Duration;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(FrpcManager::new())
        .invoke_handler(tauri::generate_handler![
            commands::app_init,
            commands::detect_frpc,
            commands::settings_save,
            commands::frpc_start,
            commands::frpc_stop,
            commands::frpc_restart,
            commands::frpc_status,
            commands::frpc_processes,
            commands::frpc_kill_pid,
            commands::logs_snapshot,
            commands::live_proxies,
            commands::config_read,
            commands::config_validate,
            commands::config_backups,
            commands::config_restore,
            commands::config_save_text,
            commands::config_save_server,
            commands::proxy_upsert,
            commands::proxy_delete,
            commands::scan_ports,
            commands::listeners_list,
            commands::probe_server,
            commands::export_logs,
            commands::logs_clear,
        ])
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let settings_path = data_dir.join("settings.json");
            let (settings, load_warning) = settings::Settings::load(&settings_path);
            if let Some(w) = &load_warning {
                eprintln!("[openfrpc] {w}");
            }
            app.manage(SettingsState {
                inner: RwLock::new(settings.clone()),
                path: settings_path,
                load_warning,
            });

            frpc::build_tray(app.handle())?;

            // 常驻守护任务：监控子进程存活并处理自动重启
            {
                let mgr = app.state::<FrpcManager>().inner().clone();
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    frpc::supervise(app_handle, mgr).await;
                });
            }

            // 开机自启动携带 --hidden 参数时静默启动
            if std::env::args().any(|a| a == "--hidden") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            if settings.auto_start_frpc {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(600)).await;
                    let s = get_settings(&app_handle);
                    let mgr = app_handle.state::<FrpcManager>();
                    if let Err(e) = frpc::start_frpc(app_handle.clone(), mgr.inner().clone(), s).await {
                        frpc::patch_status(&app_handle, &mgr, |st| {
                            st.last_error = Some(e.clone());
                        });
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let mgr = app.state::<FrpcManager>();
                let s = get_settings(app);
                if !mgr.0.exiting.load(std::sync::atomic::Ordering::Relaxed) && s.close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("OpenFrpc 初始化失败")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let mgr = app.state::<FrpcManager>();
                mgr.0.exiting.store(true, std::sync::atomic::Ordering::Relaxed);
                let s = get_settings(app);
                if s.stop_frpc_on_exit {
                    // 直接对受管子进程句柄发停止：不用可能过期的 PID 二次定位（R25 同源问题）
                    let mgr_c = mgr.inner().clone();
                    tauri::async_runtime::block_on(async move {
                        let mut g = mgr_c.0.managed.lock().await;
                        if let Some(m) = g.as_mut() {
                            m.stop_requested = true;
                            let _ = m.child.start_kill();
                        }
                    });
                }
            }
        });
}
