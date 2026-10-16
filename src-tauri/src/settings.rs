use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    #[serde(default)]
    pub frpc_path: Option<String>,
    #[serde(default)]
    pub config_path: Option<String>,
    #[serde(default)]
    pub auto_start_frpc: bool,
    #[serde(default = "d_true")]
    pub auto_restart: bool,
    #[serde(default = "d_true")]
    pub close_to_tray: bool,
    #[serde(default = "d_true")]
    pub stop_frpc_on_exit: bool,
    #[serde(default)]
    pub autostart_app: bool,
    #[serde(default = "d_lines")]
    pub log_buffer_lines: usize,
    /// light | dark | system
    #[serde(default = "d_theme")]
    pub theme: String,
    #[serde(default)]
    pub sidebar_collapsed: bool,
}

fn d_true() -> bool {
    true
}
fn d_lines() -> usize {
    5000
}
fn d_theme() -> String {
    "system".into()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            frpc_path: None,
            config_path: None,
            auto_start_frpc: false,
            auto_restart: true,
            close_to_tray: true,
            stop_frpc_on_exit: true,
            autostart_app: false,
            log_buffer_lines: 5000,
            theme: "system".into(),
            sidebar_collapsed: false,
        }
    }
}

impl Settings {
    /// 加载设置；文件损坏时不静默当全新安装——先备份原文件再回退默认值（R22）
    pub fn load(path: &Path) -> (Settings, Option<String>) {
        match fs::read_to_string(path) {
            Err(_) => (Settings::default(), None),
            Ok(s) => match serde_json::from_str::<Settings>(&s) {
                Ok(v) => (v, None),
                Err(e) => {
                    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%3f");
                    let backup = path.with_extension(format!("corrupt-{stamp}"));
                    let backup_note = if fs::copy(path, &backup).is_ok() {
                        format!("，原文件已备份为 {}", backup.display())
                    } else {
                        String::new()
                    };
                    (
                        Settings::default(),
                        Some(format!("设置文件已损坏（{e}）{backup_note}，本次使用默认设置")),
                    )
                }
            },
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // R21/R22：临时文件 + 原子替换，写盘中断不会留下半截 settings.json
        crate::config::atomic_write(path, &text).map_err(|e| format!("写入设置失败: {e}"))
    }
}

/// 全局可变设置，供命令与后台任务共享
pub struct SettingsState {
    pub inner: RwLock<Settings>,
    pub path: std::path::PathBuf,
    /// 加载设置时发现文件损坏等一次性告警，由 app_init 透出给界面
    pub load_warning: Option<String>,
}

impl SettingsState {
    pub fn get(&self) -> Settings {
        self.inner.read().unwrap().clone()
    }
}

/// 从应用状态读取当前设置的快捷方式
pub fn get_settings(app: &tauri::AppHandle) -> Settings {
    use tauri::Manager;
    app.state::<SettingsState>().get()
}
