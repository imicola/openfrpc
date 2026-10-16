use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use toml_edit::{value, Array, ArrayOfTables, Item, Table, TableLike};

use crate::settings::{get_settings, SettingsState};

/// FNV-1a 64 位内容版本号：用于「保存前文件是否被外部修改」的冲突检测（非安全用途）
pub fn text_rev(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// 同目录写临时文件后原子替换，避免写入中断在原文件上留下截断内容
pub fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{file_name}.new-{}", std::process::id()));
    fs::write(&tmp, text).map_err(|e| format!("写入临时文件失败: {e}"))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(format!("替换文件失败: {e}"))
        }
    }
}

/// 把 Item 统一成 &mut dyn TableLike（普通表与内联表都可编辑）。
/// 不能用 is_table_like() 通过后再 as_table_mut().unwrap()——内联表会返回 None 导致 panic（R01）
fn table_like_mut(item: &mut Item) -> Option<&mut dyn TableLike> {
    match item {
        Item::Table(t) => Some(t),
        Item::Value(toml_edit::Value::InlineTable(t)) => Some(t),
        _ => None,
    }
}

/// 服务端常用字段（表单编辑范围）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ServerFields {
    #[serde(default)]
    pub server_addr: String,
    #[serde(default = "d_port")]
    pub server_port: u16,
    #[serde(default = "d_token")]
    pub auth_method: String,
    #[serde(default)]
    pub auth_token: String,
    #[serde(default = "d_tcp")]
    pub transport_protocol: String,
    #[serde(default)]
    pub transport_proxy_url: String,
    /// None = 配置未显式设置（frp 0.50+ 有效缺省为启用）；Some(b) = 显式 true/false
    #[serde(default)]
    pub tls_enabled: Option<bool>,
    #[serde(default)]
    pub heartbeat_interval: Option<i64>,
    #[serde(default)]
    pub heartbeat_timeout: Option<i64>,
}

fn d_port() -> u16 {
    7000
}
fn d_token() -> String {
    "token".into()
}
fn d_tcp() -> String {
    "tcp".into()
}

/// 代理条目：覆盖 tcp/udp/http/https/stcp/xtcp/sudp 的常用字段
/// 注意：JSON 字段名统一用 ptype（前端约定）；写入 TOML 时用 "type" 键
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProxyEntry {
    pub name: String,
    pub ptype: String,
    #[serde(default = "d_localhost")]
    pub local_ip: String,
    pub local_port: u16,
    #[serde(default)]
    pub remote_port: Option<u16>,
    #[serde(default)]
    pub subdomain: Option<String>,
    #[serde(default)]
    pub custom_domains: Option<Vec<String>>,
    #[serde(default)]
    pub locations: Option<Vec<String>>,
    #[serde(default)]
    pub secret_key: Option<String>,
    /// frp 0.52+ HTTP 代理认证：httpUser / httpPassword（上游字段名，非 basicAuth 数组）
    #[serde(default)]
    pub http_user: Option<String>,
    #[serde(default)]
    pub http_pwd: Option<String>,
    #[serde(default)]
    pub use_encryption: Option<bool>,
    #[serde(default)]
    pub use_compression: Option<bool>,
    /// 只读：该条目使用 plugin 后端，localPort/localIP 由插件决定，界面编辑不改动
    #[serde(default)]
    pub plugin: Option<String>,
}

fn d_localhost() -> String {
    "127.0.0.1".into()
}

#[derive(Serialize, Clone)]
pub struct ConfigInfo {
    pub path: Option<String>,
    pub text: String,
    /// 内容版本号（FNV-1a），保存时回传用于冲突检测
    pub rev: String,
    pub server: Option<ServerFields>,
    pub proxies: Vec<ProxyEntry>,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct SaveResult {
    pub ok: bool,
    pub verified: bool,
    pub verify_output: String,
    pub text: String,
    pub rev: String,
    /// 本次保存是否已作用于运行中的 frpc（false = 已保存待重启应用）
    pub applied: bool,
    pub server: Option<ServerFields>,
    pub proxies: Vec<ProxyEntry>,
    pub warnings: Vec<String>,
}

fn parse_doc(text: &str) -> Result<toml_edit::DocumentMut, String> {
    text.parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("TOML 语法错误: {e}"))
}

pub fn parse_server(text: &str) -> Result<ServerFields, String> {
    let doc = parse_doc(text)?;
    let walk = |path: &[&str]| -> Option<&Item> {
        let mut item: Option<&Item> = None;
        for seg in path {
            item = match item {
                Some(i) => i.get(seg),
                None => doc.get(*seg),
            };
        }
        item
    };
    let g_str =
        |path: &[&str]| walk(path).and_then(|i| i.as_str()).unwrap_or("").to_string();
    let g_int = |path: &[&str]| walk(path).and_then(|i| i.as_integer());
    let g_bool = |path: &[&str]| walk(path).and_then(|i| i.as_bool());
    Ok(ServerFields {
        server_addr: g_str(&["serverAddr"]),
        server_port: g_int(&["serverPort"])
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(7000),
        auth_method: {
            let m = g_str(&["auth", "method"]);
            if m.is_empty() { "token".into() } else { m }
        },
        auth_token: g_str(&["auth", "token"]),
        transport_protocol: {
            let p = g_str(&["transport", "protocol"]);
            if p.is_empty() { "tcp".into() } else { p }
        },
        transport_proxy_url: g_str(&["transport", "proxyURL"]),
        // R04：按布尔读取；显式 false 必须保留为 Some(false)，缺省为 None
        tls_enabled: g_bool(&["transport", "tls", "enable"]),
        heartbeat_interval: g_int(&["transport", "heartbeatInterval"]),
        heartbeat_timeout: g_int(&["transport", "heartbeatTimeout"]),
    })
}

/// 从任意 TableLike（[[proxies]] 表项或内联表）解析一个代理条目
fn entry_from_table(
    t: &dyn TableLike,
    idx: usize,
    warnings: &mut Vec<String>,
) -> Option<ProxyEntry> {
    let gs = |k: &str| t.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let gi = |k: &str| t.get(k).and_then(|v| v.as_integer());
    let garr = |k: &str| -> Option<Vec<String>> {
        t.get(k)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
    };

    let name = gs("name").unwrap_or_default();
    if name.is_empty() {
        warnings.push(format!("第 {} 个代理缺少 name，已跳过", idx + 1));
        return None;
    }
    let ptype = gs("type").unwrap_or_default();
    if ptype.is_empty() {
        warnings.push(format!("代理「{name}」缺少 type，已跳过"));
        return None;
    }

    let plugin = gs("plugin");
    let local_port = match gi("localPort").and_then(|v| u16::try_from(v).ok()) {
        Some(p) => p,
        None => {
            if plugin.is_some() {
                // R19：插件后端的本地端口由插件自身决定，不视为缺字段
                0
            } else {
                warnings.push(format!("代理「{name}」缺少 localPort，按 0 处理"));
                0
            }
        }
    };

    // frp 0.52+ 嵌套字段 transport.*，兼容旧版顶层写法
    let gb_t = |k: &str| -> Option<bool> {
        t.get("transport")
            .and_then(|tr| tr.get(k))
            .and_then(|v| v.as_bool())
            .or_else(|| t.get(k).and_then(|v| v.as_bool()))
    };

    // 旧版 OpenFrpc 写过的 basicAuth = ["user:pass"] 迁移为上游字段（R13）
    let mut http_user = gs("httpUser");
    let mut http_pwd = gs("httpPassword");
    if http_user.is_none() && http_pwd.is_none() {
        if let Some(legacy) = garr("basicAuth") {
            if let Some(first) = legacy.first() {
                let (u, p) = first.split_once(':').unwrap_or((first.as_str(), ""));
                http_user = Some(u.to_string());
                http_pwd = Some(p.to_string());
                warnings.push(format!(
                    "代理「{name}」的 basicAuth 字段不是 frp 0.71 的合法配置，已按 httpUser/httpPassword 解析，保存时会自动改写"
                ));
            }
        }
    }

    Some(ProxyEntry {
        name,
        ptype,
        local_ip: gs("localIP").unwrap_or_else(|| "127.0.0.1".into()),
        local_port,
        remote_port: gi("remotePort").and_then(|v| u16::try_from(v).ok()),
        subdomain: gs("subdomain"),
        custom_domains: garr("customDomains"),
        locations: garr("locations"),
        secret_key: gs("secretKey"),
        http_user,
        http_pwd,
        use_encryption: gb_t("useEncryption"),
        use_compression: gb_t("useCompression"),
        plugin,
    })
}

pub fn parse_proxies(text: &str) -> Result<(Vec<ProxyEntry>, Vec<String>), String> {
    let doc = parse_doc(text)?;
    let mut list = Vec::new();
    let mut warnings = Vec::new();
    let Some(item) = doc.get("proxies") else {
        return Ok((list, warnings));
    };
    if let Some(arr) = item.as_array_of_tables() {
        for (idx, tbl) in arr.iter().enumerate() {
            if let Some(e) = entry_from_table(tbl, idx, &mut warnings) {
                list.push(e);
            }
        }
    } else if let Some(arr) = item.as_array() {
        // R19：等价的内联数组表示 proxies = [{...}]，支持读取；编辑写回时要求 [[proxies]] 形式
        let mut seen = 0usize;
        for (idx, el) in arr.iter().enumerate() {
            if let Some(t) = el.as_inline_table() {
                seen += 1;
                if let Some(e) = entry_from_table(t, idx, &mut warnings) {
                    list.push(e);
                }
            }
        }
        if seen > 0 {
            warnings.push(
                "proxies 使用内联数组写法，仅支持查看；如需在此编辑，请先把配置改为 [[proxies]] 数组表格式".into(),
            );
        }
    } else if !item.is_none() {
        warnings.push("proxies 字段不是 [[proxies]] 数组表，无法解析".into());
    }
    Ok((list, warnings))
}

fn ensure_table(doc: &mut toml_edit::DocumentMut, key: &str) {
    if !doc.get(key).map(|i| i.is_table_like()).unwrap_or(false) {
        doc[key] = Item::Table(Table::new());
    }
}

fn set_or_remove_str(tbl: &mut dyn TableLike, key: &str, val: &str) {
    if val.is_empty() {
        tbl.remove(key);
    } else {
        tbl.insert(key, value(val));
    }
}

fn set_opt_str(tbl: &mut dyn TableLike, key: &str, val: &Option<String>) {
    match val {
        Some(v) if !v.is_empty() => {
            tbl.insert(key, value(v.as_str()));
        }
        _ => {
            tbl.remove(key);
        }
    }
}

fn set_opt_int(tbl: &mut dyn TableLike, key: &str, val: &Option<i64>) {
    match val {
        Some(v) => {
            tbl.insert(key, value(*v));
        }
        None => {
            tbl.remove(key);
        }
    }
}

fn set_opt_bool(tbl: &mut dyn TableLike, key: &str, val: &Option<bool>) {
    match val {
        Some(b) => {
            tbl.insert(key, value(*b));
        }
        None => {
            tbl.remove(key);
        }
    }
}

fn str_array(v: &Option<Vec<String>>) -> Option<Array> {
    v.as_ref().map(|list| {
        let mut a = Array::new();
        for s in list {
            a.push(s.as_str());
        }
        a
    })
}

/// 将表单字段写回文档，保留注释与未知字段。
/// R01：全程使用 TableLike 视图，内联表（auth = {...}）同样可写，不再 unwrap panic
pub fn apply_server_fields(text: &str, f: &ServerFields) -> Result<String, String> {
    let mut doc = parse_doc(text)?;
    doc["serverAddr"] = value(f.server_addr.as_str());
    doc["serverPort"] = value(i64::from(f.server_port));

    ensure_table(&mut doc, "auth");
    {
        let auth = table_like_mut(doc.get_mut("auth").unwrap())
            .ok_or("auth 字段不是表，无法写入")?;
        auth.insert("method", value(f.auth_method.as_str()));
        auth.insert("token", value(f.auth_token.as_str()));
    }

    ensure_table(&mut doc, "transport");
    {
        let t = table_like_mut(doc.get_mut("transport").unwrap())
            .ok_or("transport 字段不是表，无法写入")?;
        t.insert("protocol", value(f.transport_protocol.as_str()));
        set_or_remove_str(t, "proxyURL", &f.transport_proxy_url);
        // R04：显式写入 true/false；仅在「从未设置」时保持缺省
        match f.tls_enabled {
            Some(b) => {
                if !t.get("tls").map(|i| i.is_table_like()).unwrap_or(false) {
                    t.insert("tls", Item::Table(Table::new()));
                }
                let tls = table_like_mut(t.get_mut("tls").unwrap())
                    .ok_or("transport.tls 字段不是表，无法写入")?;
                tls.insert("enable", value(b));
            }
            None => {
                if let Some(tls) = t.get_mut("tls").and_then(table_like_mut) {
                    tls.remove("enable");
                }
            }
        }
        set_opt_int(t, "heartbeatInterval", &f.heartbeat_interval);
        set_opt_int(t, "heartbeatTimeout", &f.heartbeat_timeout);
    }
    Ok(doc.to_string())
}

fn proxies_array_mut(doc: &mut toml_edit::DocumentMut) -> Result<&mut ArrayOfTables, String> {
    if doc.get("proxies").is_none() {
        doc["proxies"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    doc.get_mut("proxies")
        .and_then(|i| i.as_array_of_tables_mut())
        .ok_or_else(|| {
            "proxies 当前是内联数组写法，OpenFrpc 只能编辑 [[proxies]] 数组表格式；请在源码页改为 [[proxies]] 后重试".to_string()
        })
}

fn tbl_name(t: &Table) -> &str {
    t.get("name").and_then(|v| v.as_str()).unwrap_or("")
}

fn apply_entry(tbl: &mut Table, e: &ProxyEntry) {
    tbl.insert("name", value(e.name.as_str()));
    tbl.insert("type", value(e.ptype.as_str()));
    if e.plugin.is_some() {
        // R19：插件后端的本地地址/端口由插件决定，编辑其他字段时不改动
    } else {
        tbl.insert("localIP", value(e.local_ip.as_str()));
        tbl.insert("localPort", value(i64::from(e.local_port)));
    }
    match e.remote_port {
        Some(p) => {
            tbl.insert("remotePort", value(i64::from(p)));
        }
        None => {
            tbl.remove("remotePort");
        }
    }
    set_opt_str(tbl, "subdomain", &e.subdomain);
    match str_array(&e.custom_domains) {
        Some(a) => {
            tbl.insert("customDomains", value(a));
        }
        None => {
            tbl.remove("customDomains");
        }
    }
    match str_array(&e.locations) {
        Some(a) => {
            tbl.insert("locations", value(a));
        }
        None => {
            tbl.remove("locations");
        }
    }
    set_opt_str(tbl, "secretKey", &e.secret_key);
    set_opt_str(tbl, "httpUser", &e.http_user);
    set_opt_str(tbl, "httpPassword", &e.http_pwd);
    // frp 0.52+ 为代理级嵌套字段 transport.useEncryption / transport.useCompression
    if !tbl.get("transport").map(|i| i.is_table_like()).unwrap_or(false) {
        tbl.insert("transport", Item::Table(Table::new()));
    }
    {
        let transport = table_like_mut(tbl.get_mut("transport").unwrap())
            .ok_or("transport 字段不是表，无法写入")
            .expect("上方已确保为 table-like");
        set_opt_bool(transport, "useEncryption", &e.use_encryption);
        set_opt_bool(transport, "useCompression", &e.use_compression);
    }
    // 清理旧版顶层字段与已被上游移除的 basicAuth（frp 0.71 会因未知字段拒绝加载）
    tbl.remove("useEncryption");
    tbl.remove("useCompression");
    tbl.remove("basicAuth");

    // R14：跨类型迁移时清理不属于新类型的字段，避免残留非法键
    let is_web = matches!(e.ptype.as_str(), "http" | "https");
    let is_secret = matches!(e.ptype.as_str(), "stcp" | "xtcp" | "sudp");
    if !is_web {
        tbl.remove("subdomain");
        tbl.remove("customDomains");
    }
    if e.ptype != "http" {
        tbl.remove("locations");
        tbl.remove("httpUser");
        tbl.remove("httpPassword");
    }
    if !is_secret {
        tbl.remove("secretKey");
    }
}

/// 代理写入。语义（R12）：
/// - old_name = None → 创建：目标名已存在必须失败，绝不覆盖；
/// - old_name = Some(old) → 更新/重命名：目标条目不存在报冲突，新名称不得与其他条目撞名
pub fn upsert_proxy(text: &str, old_name: Option<&str>, entry: &ProxyEntry) -> Result<String, String> {
    let mut doc = parse_doc(text)?;
    let arr = proxies_array_mut(&mut doc)?;
    match old_name {
        None => {
            if arr.iter().any(|t| tbl_name(t) == entry.name) {
                return Err(format!(
                    "已存在同名代理「{}」；如需修改请使用编辑，或换一个名称",
                    entry.name
                ));
            }
            let mut tbl = Table::new();
            apply_entry(&mut tbl, entry);
            arr.push(tbl);
        }
        Some(old) => {
            let idx = arr
                .iter()
                .position(|t| tbl_name(t) == old)
                .ok_or_else(|| {
                    format!("未找到要编辑的代理「{old}」（文件可能已被外部修改），请刷新后重试")
                })?;
            if old != entry.name
                && arr
                    .iter()
                    .enumerate()
                    .any(|(i, t)| i != idx && tbl_name(t) == entry.name)
            {
                return Err(format!("已存在同名代理「{}」，重命名冲突", entry.name));
            }
            apply_entry(arr.get_mut(idx).unwrap(), entry);
        }
    }
    Ok(doc.to_string())
}

pub fn delete_proxy(text: &str, name: &str) -> Result<String, String> {
    let mut doc = parse_doc(text)?;
    let arr = proxies_array_mut(&mut doc)?;
    let idx = arr.iter().position(|t| tbl_name(t) == name);
    match idx {
        Some(i) => {
            arr.remove(i);
            Ok(doc.to_string())
        }
        None => Err(format!("未找到名为「{name}」的代理")),
    }
}

/// 时间戳备份：毫秒精度命名 + 已存在时追加序号（R06：同一秒多次保存不得互相覆盖）；
/// 命名沿用 frpc.toml.bak-YYYYMMDD-HHMMSS 习惯；保留最近 10 份
fn backup_with_rotation(path: &Path) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Ok(path.to_path_buf());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("frpc.toml");
    let mut backup = path.with_file_name(format!("{file_name}.bak-{stamp}"));
    for n in 1..50u32 {
        if !backup.is_file() {
            break;
        }
        backup = path.with_file_name(format!("{file_name}.bak-{stamp}-{n}"));
    }
    fs::copy(path, &backup).map_err(|e| format!("备份失败: {e}"))?;

    let prefix = format!("{file_name}.bak-");
    let mut backups: Vec<PathBuf> = fs::read_dir(path.parent().unwrap_or(path))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.starts_with(&prefix))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();
    backups.sort();
    while backups.len() > 10 {
        let oldest = backups.remove(0);
        let _ = fs::remove_file(oldest);
    }
    Ok(backup)
}

/// 用 frpc verify 校验文本（写入数据目录临时文件，不动原文件）
fn verify_text(app: &AppHandle, text: &str) -> Result<(bool, String), String> {
    let settings = get_settings(app);
    let frpc = match settings.frpc_path {
        Some(ref p) if Path::new(p).is_file() => p.clone(),
        _ => return Ok((false, "未设置 frpc.exe，跳过语法校验".into())),
    };
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?;
    let _ = fs::create_dir_all(&data_dir);
    let tmp = data_dir.join("frpdeck-verify.toml");
    fs::write(&tmp, text).map_err(|e| format!("写入临时校验文件失败: {e}"))?;
    let out = crate::listeners::run_hidden_capture(
        &frpc,
        &["verify", "-c", tmp.to_string_lossy().as_ref()],
        10,
    );
    let _ = fs::remove_file(&tmp);
    match out {
        Some(o) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let pass = o.status.success() || combined.contains("syntax check pass");
            Ok((pass, combined.trim().to_string()))
        }
        None => Ok((false, "无法执行 frpc verify（进程启动失败或超时未退出）".into())),
    }
}

/// 通用落盘：语法预检 → 外部修改冲突检测（R21）→ 校验 → 备份 → 原子写入 → 返回新状态
pub fn save_text(
    app: &AppHandle,
    text: &str,
    validate: bool,
    expected_rev: Option<&str>,
) -> Result<SaveResult, String> {
    // 先保证语法可解析，避免把损坏内容写回
    parse_doc(text)?;
    let settings = app.state::<SettingsState>().get();
    let path = settings
        .config_path
        .clone()
        .filter(|p| Path::new(p).is_file())
        .ok_or("未设置配置文件路径")?;

    // R21：调用方带着「打开时的内容版本号」，与当前磁盘不一致则拒绝盲写
    if let Some(expected) = expected_rev {
        let disk = fs::read_to_string(&path).unwrap_or_default();
        if text_rev(&disk) != expected {
            return Err(
                "配置文件在保存前已被外部修改（内容与打开时不一致）。请先复制你的修改，刷新加载最新内容后再保存"
                    .into(),
            );
        }
    }

    let (verified, verify_output) = if validate {
        let (ok, out) = verify_text(app, text)?;
        if !ok && !out.contains("跳过") {
            return Err(format!("frpc 校验未通过：\n{out}"));
        }
        (ok, out)
    } else {
        (false, String::new())
    };

    backup_with_rotation(Path::new(&path))?;
    atomic_write(Path::new(&path), text).map_err(|e| format!("写入配置失败: {e}"))?;

    build_save_result(&path, text, verified, verify_output)
}

pub fn read_config(app: &AppHandle) -> Result<ConfigInfo, String> {
    let settings = app.state::<SettingsState>().get();
    let Some(path) = settings.config_path.clone() else {
        return Ok(ConfigInfo {
            path: None,
            text: String::new(),
            rev: text_rev(""),
            server: None,
            proxies: Vec::new(),
            warnings: Vec::new(),
        });
    };
    if !Path::new(&path).is_file() {
        return Ok(ConfigInfo {
            path: Some(path),
            text: String::new(),
            rev: text_rev(""),
            server: None,
            proxies: Vec::new(),
            warnings: vec!["配置文件不存在".into()],
        });
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {e}"))?;
    build_config_info(Some(path), &text)
}

pub fn build_config_info(path: Option<String>, text: &str) -> Result<ConfigInfo, String> {
    let server = parse_server(text).ok();
    let (proxies, warnings) =
        parse_proxies(text).unwrap_or((Vec::new(), vec!["解析 proxies 失败".into()]));
    Ok(ConfigInfo {
        path,
        rev: text_rev(text),
        text: text.to_string(),
        server,
        proxies,
        warnings,
    })
}

fn build_save_result(
    path: &str,
    text: &str,
    verified: bool,
    verify_output: String,
) -> Result<SaveResult, String> {
    let info = build_config_info(Some(path.to_string()), text)?;
    Ok(SaveResult {
        ok: true,
        verified,
        verify_output,
        text: info.text,
        rev: info.rev,
        applied: false, // 由命令层根据运行状态填充
        server: info.server,
        proxies: info.proxies,
        warnings: info.warnings,
    })
}

/// 仅校验文本语法（frpc verify + TOML 解析），不落盘
pub fn validate_only(app: &AppHandle, text: &str) -> Result<(bool, String), String> {
    parse_doc(text)?;
    verify_text(app, text)
}

#[derive(Serialize, Clone)]
pub struct BackupInfo {
    pub name: String,
    pub path: String,
    pub mtime: String,
}

/// 列出配置同目录下的时间戳备份（新→旧）
pub fn list_backups(app: &AppHandle) -> Vec<BackupInfo> {
    let settings = app.state::<SettingsState>().get();
    let Some(cfg) = settings.config_path else {
        return Vec::new();
    };
    let path = Path::new(&cfg);
    let Some(dir) = path.parent() else { return Vec::new(); };
    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else { return Vec::new(); };
    let prefix = format!("{file_name}.bak-");
    let mut out: Vec<BackupInfo> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.starts_with(&prefix))
                        .unwrap_or(false)
                })
                .filter_map(|p| {
                    let name = p.file_name()?.to_str()?.to_string();
                    let mtime = fs::metadata(&p)
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| {
                            chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                                .map(|dt| {
                                    dt.with_timezone(&chrono::Local)
                                        .format("%Y-%m-%d %H:%M:%S")
                                        .to_string()
                                })
                                .unwrap_or_default()
                        })
                        .unwrap_or_default();
                    Some(BackupInfo {
                        name,
                        path: p.to_string_lossy().to_string(),
                        mtime,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// 恢复备份：当前配置先按惯例备份，再把备份内容写入
pub fn restore_backup(app: &AppHandle, backup_path: &str) -> Result<SaveResult, String> {
    let text = fs::read_to_string(backup_path).map_err(|e| format!("读取备份失败: {e}"))?;
    save_text(app, &text, true, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# 顶部注释
serverAddr = "1.2.3.4"
serverPort = 7000

[auth]
method = "token"
token = "abc"

[[proxies]]
name = "demo"
type = "http"
localIP = "127.0.0.1"
localPort = 8770
subdomain = "dev"
"#;

    fn sample_entry(name: &str, ptype: &str, local_port: u16) -> ProxyEntry {
        ProxyEntry {
            name: name.into(),
            ptype: ptype.into(),
            local_ip: "127.0.0.1".into(),
            local_port,
            remote_port: Some(6022),
            subdomain: None,
            custom_domains: None,
            locations: None,
            secret_key: None,
            http_user: None,
            http_pwd: None,
            use_encryption: Some(true),
            use_compression: None,
            plugin: None,
        }
    }

    #[test]
    fn roundtrip_preserves_comments() {
        let f = ServerFields {
            server_addr: "5.6.7.8".into(),
            server_port: 7100,
            auth_method: "token".into(),
            auth_token: "xyz".into(),
            transport_protocol: "tcp".into(),
            transport_proxy_url: String::new(),
            tls_enabled: None,
            heartbeat_interval: None,
            heartbeat_timeout: None,
        };
        let out = apply_server_fields(SAMPLE, &f).unwrap();
        assert!(out.contains("# 顶部注释"));
        assert!(out.contains("5.6.7.8"));
        assert!(!out.contains("proxyURL"));
    }

    #[test]
    fn upsert_then_delete() {
        let e = sample_entry("ssh", "tcp", 22);
        let t1 = upsert_proxy(SAMPLE, None, &e).unwrap();
        assert!(t1.contains("remotePort = 6022"));
        assert!(t1.contains("[proxies.transport]"));
        assert!(t1.contains("useEncryption = true"));
        // 顶层（transport 子表之前）不得出现旧字段
        let (before_transport, _) = t1.split_once("[proxies.transport]").unwrap();
        assert!(!before_transport.contains("useEncryption"));
        let t2 = delete_proxy(&t1, "ssh").unwrap();
        assert!(!t2.contains("6022"));
        // demo 仍在
        assert!(t2.contains("demo"));
    }

    #[test]
    fn parse_proxies_works() {
        let (list, warn) = parse_proxies(SAMPLE).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "demo");
        assert_eq!(list[0].ptype, "http");
        assert_eq!(list[0].subdomain.as_deref(), Some("dev"));
        assert!(warn.is_empty());
    }

    #[test]
    fn entry_serializes_as_ptype() {
        let j = serde_json::to_value(sample_entry("a", "tcp", 1)).unwrap();
        assert!(j.get("ptype").is_some());
        assert!(j.get("type").is_none());
    }

    // ---------- R01：合法内联表不得 panic ----------

    #[test]
    fn inline_auth_and_tls_tables_do_not_panic() {
        let cfg = r#"serverAddr = "1.2.3.4"
serverPort = 7000
auth = { method = "token", token = "x" }
transport = { protocol = "tcp", tls = { enable = true } }
"#;
        let f = ServerFields {
            server_addr: "5.6.7.8".into(),
            server_port: 7000,
            auth_method: "token".into(),
            auth_token: "y".into(),
            transport_protocol: "kcp".into(),
            transport_proxy_url: String::new(),
            tls_enabled: Some(false),
            heartbeat_interval: None,
            heartbeat_timeout: None,
        };
        let out = apply_server_fields(cfg, &f).unwrap();
        assert!(out.contains("5.6.7.8"));
        assert!(out.contains("kcp"));
        // 内联 auth 表保留内联形态且值被更新
        assert!(out.contains("token = \"y\""));
        // 内联 tls 表写入显式 false
        let parsed = parse_server(&out).unwrap();
        assert_eq!(parsed.tls_enabled, Some(false));
    }

    #[test]
    fn inline_proxy_transport_table_writable() {
        let cfg = r#"serverAddr = "1.2.3.4"
[[proxies]]
name = "a"
type = "tcp"
localPort = 22
transport = { useEncryption = true }
"#;
        let mut e = sample_entry("a", "tcp", 22);
        e.use_encryption = Some(false);
        let out = upsert_proxy(cfg, Some("a"), &e).unwrap();
        let (list, _) = parse_proxies(&out).unwrap();
        assert_eq!(list[0].use_encryption, Some(false));
    }

    // ---------- R04：TLS 三态往返 ----------

    #[test]
    fn tls_true_false_absent_roundtrip() {
        // true → 保留 true
        let f = base_fields(Some(true));
        let out = apply_server_fields("serverAddr=\"a\"\nserverPort=1\n", &f).unwrap();
        assert!(out.contains("enable = true"));
        // false → 必须显式写入 false（不得删除回落为缺省）
        let f = base_fields(Some(false));
        let out = apply_server_fields("serverAddr=\"a\"\nserverPort=1\n", &f).unwrap();
        assert!(out.contains("enable = false"));
        let back = parse_server(&out).unwrap();
        assert_eq!(back.tls_enabled, Some(false));
        // 缺省 → 解析为 None
        let back = parse_server("serverAddr=\"a\"\nserverPort=1\n").unwrap();
        assert_eq!(back.tls_enabled, None);
    }

    fn base_fields(tls: Option<bool>) -> ServerFields {
        ServerFields {
            server_addr: "a".into(),
            server_port: 1,
            auth_method: "token".into(),
            auth_token: "t".into(),
            transport_protocol: "tcp".into(),
            transport_proxy_url: String::new(),
            tls_enabled: tls,
            heartbeat_interval: None,
            heartbeat_timeout: None,
        }
    }

    // ---------- R12：创建/更新/重命名语义 ----------

    #[test]
    fn create_existing_name_fails_without_touching_entry() {
        // 已有 ssh→22，用「新建」提交同名 ssh→3306 必须失败，且原条目不被改动
        let cfg = r#"serverAddr = "1.2.3.4"
[[proxies]]
name = "ssh"
type = "tcp"
localPort = 22
"#;
        let mut e = sample_entry("ssh", "tcp", 3306);
        e.remote_port = None;
        let err = upsert_proxy(cfg, None, &e).unwrap_err();
        assert!(err.contains("已存在同名代理"));
        let (list, _) = parse_proxies(cfg).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].local_port, 22);
    }

    #[test]
    fn rename_to_existing_fails() {
        let cfg = r#"serverAddr = "1.2.3.4"
[[proxies]]
name = "ssh"
type = "tcp"
localPort = 22
[[proxies]]
name = "mysql"
type = "tcp"
localPort = 3306
"#;
        let e = sample_entry("mysql", "tcp", 22);
        let err = upsert_proxy(cfg, Some("ssh"), &e).unwrap_err();
        assert!(err.contains("重命名冲突"));
    }

    #[test]
    fn update_missing_target_fails_not_falls_back_to_create() {
        let e = sample_entry("brand-new", "tcp", 22);
        let err = upsert_proxy(SAMPLE, Some("gone"), &e).unwrap_err();
        assert!(err.contains("未找到要编辑的代理"));
    }

    // ---------- R13：HTTP 认证用上游字段 ----------

    #[test]
    fn http_auth_written_as_upstream_keys() {
        let mut e = sample_entry("web", "http", 80);
        e.http_user = Some("admin".into());
        e.http_pwd = Some("secret".into());
        let out = upsert_proxy(SAMPLE, None, &e).unwrap();
        assert!(out.contains("httpUser = \"admin\""));
        assert!(out.contains("httpPassword = \"secret\""));
        assert!(!out.contains("basicAuth"));
    }

    #[test]
    fn legacy_basicauth_parsed_and_migrated() {
        let cfg = r#"serverAddr = "1.2.3.4"
[[proxies]]
name = "web"
type = "http"
localPort = 80
basicAuth = ["admin:secret"]
"#;
        let (list, warn) = parse_proxies(cfg).unwrap();
        assert_eq!(list[0].http_user.as_deref(), Some("admin"));
        assert_eq!(list[0].http_pwd.as_deref(), Some("secret"));
        assert!(warn.iter().any(|w| w.contains("basicAuth")));
        // 保存（更新同名）后 basicAuth 被替换为上游字段
        let out = upsert_proxy(cfg, Some("web"), &list[0]).unwrap();
        assert!(out.contains("httpUser = \"admin\""));
        assert!(!out.contains("basicAuth"));
    }

    // ---------- R14：类型切换清理 ----------

    #[test]
    fn switching_http_to_tcp_cleans_type_specific_fields() {
        let cfg = r#"serverAddr = "1.2.3.4"
[[proxies]]
name = "web"
type = "http"
localPort = 8770
subdomain = "dev"
locations = ["/api"]
httpUser = "admin"
httpPassword = "secret"
"#;
        let mut e = sample_entry("web", "tcp", 8770);
        e.subdomain = None;
        let out = upsert_proxy(cfg, Some("web"), &e).unwrap();
        let (list, _) = parse_proxies(&out).unwrap();
        assert_eq!(list[0].ptype, "tcp");
        assert_eq!(list[0].subdomain, None);
        assert_eq!(list[0].http_user, None);
        assert!(!out.contains("httpUser"));
        assert!(!out.contains("subdomain"));
        assert!(!out.contains("locations"));
    }

    // ---------- R19：内联数组与插件条目 ----------

    #[test]
    fn inline_proxies_array_parsed_with_warning() {
        let cfg = "serverAddr = \"1.2.3.4\"\nproxies = [{name=\"ssh\",type=\"tcp\",localPort=22,remotePort=6022}]\n";
        let (list, warn) = parse_proxies(cfg).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "ssh");
        assert!(warn.iter().any(|w| w.contains("内联数组")));
        // 编辑写回应报错（提示先转换格式），而不是静默丢数据
        assert!(upsert_proxy(cfg, None, &sample_entry("x", "tcp", 1)).is_err());
    }

    #[test]
    fn plugin_entry_keeps_local_port_untouched() {
        let cfg = r#"serverAddr = "1.2.3.4"
[[proxies]]
name = "static"
type = "tcp"
remotePort = 6000
plugin = "static_file"
pluginLocalPath = "/srv"
"#;
        let (list, warn) = parse_proxies(cfg).unwrap();
        assert_eq!(list[0].plugin.as_deref(), Some("static_file"));
        assert_eq!(list[0].local_port, 0);
        assert!(!warn.iter().any(|w| w.contains("缺少 localPort")));
        // 更新该条目（如改 remotePort）不改动插件相关字段
        let mut e = list[0].clone();
        e.remote_port = Some(6001);
        let out = upsert_proxy(cfg, Some("static"), &e).unwrap();
        assert!(out.contains("plugin = \"static_file\""));
        assert!(out.contains("pluginLocalPath"));
        assert!(!out.contains("localPort"));
        assert!(out.contains("remotePort = 6001"));
    }

    // ---------- R06：备份不互相覆盖 ----------

    #[test]
    fn rapid_backups_keep_both_versions() {
        let dir = std::env::temp_dir().join(format!("frpdeck-test-bak-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("frpc.toml");
        fs::write(&cfg, "original").unwrap();
        let _ = backup_with_rotation(&cfg).unwrap();
        fs::write(&cfg, "modified").unwrap();
        let _ = backup_with_rotation(&cfg).unwrap();
        let mut contents: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("frpc.toml.bak-"))
            .filter_map(|e| fs::read_to_string(e.path()).ok())
            .collect();
        contents.sort();
        assert_eq!(contents, vec!["modified".to_string(), "original".to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }

    // ---------- R21：内容版本与原子写 ----------

    #[test]
    fn text_rev_changes_with_content() {
        assert_ne!(text_rev("a"), text_rev("b"));
        assert_eq!(text_rev("a"), text_rev("a"));
        assert_eq!(text_rev("").len(), 16);
    }

    #[test]
    fn atomic_write_replaces_content() {
        let dir = std::env::temp_dir().join(format!("frpdeck-test-aw-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.toml");
        fs::write(&p, "old").unwrap();
        atomic_write(&p, "new-content").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new-content");
        // 不残留临时文件
        let leftovers = fs::read_dir(&dir).unwrap().filter(|e| {
            e.as_ref().map(|x| x.file_name().to_string_lossy().starts_with(".")).unwrap_or(false)
        }).count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
