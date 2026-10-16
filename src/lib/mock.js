// 浏览器环境下的后端模拟：结构与 Rust 命令一致，用于无 Tauri 的视觉验证
const emitters = new Map();

function emitLocal(event, payload) {
  (emitters.get(event) || []).forEach((h) => h({ event, payload }));
}

export function mockListen(event, handler) {
  if (!emitters.has(event)) emitters.set(event, []);
  emitters.get(event).push(handler);
  return Promise.resolve(() => {
    const arr = emitters.get(event) || [];
    const i = arr.indexOf(handler);
    if (i >= 0) arr.splice(i, 1);
  });
}

const DEMO_CONFIG = `# frpc 配置（OpenFrpc 演示数据）
transport.protocol = "tcp"

serverAddr = "203.0.113.10"
serverPort = 7000

auth.method = "token"
auth.token = "demo-token"

[[proxies]]
name = "dev-web"
type = "http"
localIP = "127.0.0.1"
localPort = 8770
subdomain = "dev"

[[proxies]]
name = "ssh"
type = "tcp"
localIP = "127.0.0.1"
localPort = 22
remotePort = 6022
`;

const state = {
  phase: "running",
  connected: true,
  pid: 42132,
  started_epoch: Math.floor(Date.now() / 1000) - 7382,
  run_id: "670f961effdf576c",
  logs: [],
  seq: 1,
  settings: {
    frpc_path: "D:\\apps\\frp\\frp_0.71.1_windows_amd64\\frpc.exe",
    config_path: "D:\\apps\\frp\\frp_0.71.1_windows_amd64\\frpc.toml",
    auto_start_frpc: false,
    auto_restart: true,
    close_to_tray: true,
    stop_frpc_on_exit: true,
    autostart_app: false,
    log_buffer_lines: 5000,
    theme: "light",
    sidebar_collapsed: false,
  },
  live: [
    { name: "dev-web", ptype: "http", status: "running", err: "", local_addr: "127.0.0.1:8770", remote_addr: "" },
    { name: "ssh", ptype: "tcp", status: "running", err: "", local_addr: "127.0.0.1:22", remote_addr: "203.0.113.10:6022" },
  ],
};

function mklog(level, src, msg) {
  const d = new Date();
  const p = (n, w = 2) => String(n).padStart(w, "0");
  const ts = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
  return { id: state.seq++, ts, level, src, msg };
}

const SEED_LOGS = [
  ["I", "client/service.go:335", "try to connect to server..."],
  ["I", "client/service.go:355", "login to server success, get run id [670f961effdf576c]"],
  ["I", "proxy/proxy_manager.go:183", "proxy added: [dev-web ssh]"],
  ["I", "client/control.go:174", "[dev-web] start proxy success"],
  ["I", "client/control.go:174", "[ssh] start proxy success"],
  ["I", "client/control.go:305", "[dev-web] get a new work connection: [1.2.3.4:57894]"],
  ["W", "client/service.go:130", "connect to server error: dial tcp 203.0.113.10:7000: i/o timeout"],
  ["I", "client/service.go:335", "try to connect to server..."],
  ["I", "client/service.go:355", "login to server success, get run id [670f961effdf576c]"],
];
SEED_LOGS.forEach(([lv, src, msg]) => state.logs.push(mklog(lv, src, msg)));

let timer = null;
function startStream() {
  stopStream();
  timer = setInterval(() => {
    if (state.phase !== "stopped") {
      const l = mklog("I", "client/control.go:305", `[dev-web] get a new work connection: [198.51.100.${Math.floor(Math.random() * 200)}:${40000 + Math.floor(Math.random() * 20000)}]`);
      state.logs.push(l);
      emitLocal("frpc:log", l);
    }
  }, 2600);
}
function stopStream() {
  if (timer) clearInterval(timer);
  timer = null;
}
startStream();

function snap() {
  return {
    phase: state.phase,
    pid: state.phase === "stopped" ? null : state.pid,
    started_at: null,
    started_epoch: state.phase === "stopped" ? null : state.started_epoch,
    uptime_secs: state.phase === "stopped" ? 0 : Math.floor(Date.now() / 1000) - state.started_epoch,
    connected: state.connected,
    run_id: state.run_id,
    admin_port: state.phase === "stopped" ? null : 15907,
    last_error: null,
    restart_attempts: 0,
    user_prefix: null,
    pending_apply: false,
  };
}

const g2 = (text, k) => { const m = text.match(new RegExp(`^${k}\\s*=\\s*"([^"]*)"`, "m")); return m ? m[1] : ""; };
const gi2 = (text, k) => { const m = text.match(new RegExp(`^${k}\\s*=\\s*(\\d+)`, "m")); return m ? Number(m[1]) : null; };

// 与后端一致的 FNV-1a 内容版本号（演示用）
function fnv(text) {
  let h = 0xcbf29ce484222325n;
  for (let i = 0; i < text.length; i++) {
    h ^= BigInt(text.charCodeAt(i));
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, "0");
}

function serverOf(text) {
  return {
    server_addr: g2(text, "serverAddr"),
    server_port: gi2(text, "serverPort") || 7000,
    auth_method: "token",
    auth_token: g2(text, "auth.token") || g2(text, "token"),
    transport_protocol: g2(text, "transport.protocol") || "tcp",
    transport_proxy_url: g2(text, "transport.proxyURL"),
    tls_enabled: null,
    heartbeat_interval: null,
    heartbeat_timeout: null,
  };
}

function parseProxies(text) {
  const out = [];
  const re = /\[\[proxies\]\]([\s\S]*?)(?=\n\[\[|\n*$)/g;
  let m;
  while ((m = re.exec(text))) {
    const block = m[1];
    const g = (k) => { const mm = block.match(new RegExp(`^${k}\\s*=\\s*"([^"]*)"`, "m")); return mm ? mm[1] : undefined; };
    const gi = (k) => { const mm = block.match(new RegExp(`^${k}\\s*=\\s*(\\d+)`, "m")); return mm ? Number(mm[1]) : undefined; };
    out.push({
      name: g("name") || "?",
      ptype: g("type") || "tcp",
      local_ip: g("localIP") || "127.0.0.1",
      local_port: gi("localPort") || 0,
      remote_port: gi("remotePort") !== undefined ? gi("remotePort") : null,
      subdomain: g("subdomain") || null,
      custom_domains: null,
      locations: null,
      secret_key: g("secretKey") || null,
      http_user: g("httpUser") || null,
      http_pwd: g("httpPassword") || null,
      use_encryption: null,
      use_compression: null,
      plugin: g("plugin") || null,
    });
  }
  return out;
}

function saveResultOf(text) {
  return {
    ok: true,
    verified: true,
    verify_output: "syntax check pass",
    text,
    rev: fnv(text),
    applied: state.phase === "stopped",
    server: serverOf(text),
    proxies: parseProxies(text),
    warnings: [],
  };
}

export async function mockInvoke(cmd, args) {
  await new Promise((r) => setTimeout(r, 120));
  switch (cmd) {
    case "app_init":
      return {
        settings: state.settings,
        settings_warning: null,
        detected: { path: state.settings.frpc_path, version: "0.71.1" },
        app_version: "1.0.0",
        data_dir: "C:\\Users\\you\\AppData\\Roaming\\com.zhaochenxi.openfrpc",
        config: { path: state.settings.config_path, text: DEMO_CONFIG, rev: fnv(DEMO_CONFIG), server: serverOf(DEMO_CONFIG), proxies: parseProxies(DEMO_CONFIG), warnings: [] },
        status: snap(),
        logs: state.logs.slice(-200),
        live: state.live,
        frpc_processes: [{ pid: state.pid, name: "frpc.exe", exe: state.settings.frpc_path, start: "2026-10-16 15:18:31" }],
      };
    case "detect_frpc":
      return { path: state.settings.frpc_path, version: "0.71.1" };
    case "settings_save":
      state.settings = { ...state.settings, ...args.settings };
      return null;
    case "frpc_start": {
      state.phase = "starting";
      emitLocal("frpc:status", snap());
      setTimeout(() => {
        state.phase = "running";
        state.connected = true;
        state.started_epoch = Math.floor(Date.now() / 1000);
        state.run_id = "670f961effdf576c";
        const l = mklog("I", "client/service.go:355", "login to server success, get run id [670f961effdf576c]");
        state.logs.push(l);
        emitLocal("frpc:log", l);
        emitLocal("frpc:status", snap());
        emitLocal("frpc:proxies", state.live);
      }, 900);
      startStream();
      return null;
    }
    case "frpc_stop":
      state.phase = "stopped";
      state.connected = false;
      stopStream();
      emitLocal("frpc:status", snap());
      emitLocal("frpc:proxies", []);
      return "frpc 已停止（PID 42132）";
    case "frpc_restart":
      return "frpc 已重启（新 PID 42133）";
    case "frpc_status":
      return snap();
    case "frpc_processes":
      return state.phase === "stopped" ? [] : [{ pid: state.pid, name: "frpc.exe", exe: state.settings.frpc_path, start: "2026-10-16 15:18:31" }];
    case "frpc_kill_pid":
    case "kill_pid":
      return null;
    case "logs_snapshot":
      return state.logs.slice(-200);
    case "live_proxies":
      return state.live;
    case "logs_clear":
      state.logs = [];
      return null;
    case "export_logs":
      return state.logs.length;
    case "config_read":
      return { path: state.settings.config_path, text: DEMO_CONFIG, rev: fnv(DEMO_CONFIG), server: serverOf(DEMO_CONFIG), proxies: parseProxies(DEMO_CONFIG), warnings: [] };
    case "config_validate":
      return { ok: true, output: "syntax check pass" };
    case "config_backups":
      return [
        { name: "frpc.toml.bak-20261016-150100", path: "D:\\apps\\frp\\frp_0.71.1_windows_amd64\\frpc.toml.bak-20261016-150100", mtime: "2026-10-16 15:01:00" },
        { name: "frpc.toml.bak-20261016-142200", path: "D:\\apps\\frp\\frp_0.71.1_windows_amd64\\frpc.toml.bak-20261016-142200", mtime: "2026-10-16 14:22:00" },
      ];
    case "config_restore":
      return saveResultOf(DEMO_CONFIG);
    case "config_save_text": {
      if (args.expectedRev && args.expectedRev !== fnv(DEMO_CONFIG)) {
        throw new Error("配置文件在保存前已被外部修改（内容与打开时不一致）。请先复制你的修改，刷新加载最新内容后再保存");
      }
      return saveResultOf(args.text);
    }
    case "config_save_server":
      return saveResultOf(DEMO_CONFIG);
    case "proxy_upsert": {
      const names = parseProxies(DEMO_CONFIG).map((p) => p.name);
      if (!args.oldName && names.includes(args.entry.name)) {
        throw new Error(`已存在同名代理「${args.entry.name}」；如需修改请使用编辑，或换一个名称`);
      }
      return saveResultOf(DEMO_CONFIG);
    }
    case "proxy_delete":
      return saveResultOf(DEMO_CONFIG);
    case "scan_ports": {
      const total = args.ports.length;
      for (let d = 20; d <= total; d += 20) {
        setTimeout(() => emitLocal("scan:progress", { task: args.task ?? null, done: d, total }), (d / total) * 1800);
      }
      const openSet = new Set([80, 443, 3306, 7000, 8080, 22]);
      return args.ports
        .map((p) => ({ port: p, open: openSet.has(p), latency_ms: openSet.has(p) ? 3 + Math.floor(Math.random() * 120) : null }))
        .sort((a, b) => a.port - b.port);
    }
    case "listeners_list":
      return [
        { proto: "TCP", local: "0.0.0.0", port: 135, state: "LISTEN", pid: 1084, pname: "svchost.exe" },
        { proto: "TCP", local: "127.0.0.1", port: 7897, state: "LISTEN", pid: 12032, pname: "clash-verge.exe" },
        { proto: "TCP", local: "127.0.0.1", port: 8770, state: "LISTEN", pid: 15420, pname: "node.exe" },
        { proto: "TCP", local: "0.0.0.0", port: 445, state: "LISTEN", pid: 4, pname: "System" },
        { proto: "TCP", local: "127.0.0.1", port: 1420, state: "LISTEN", pid: 22110, pname: "node.exe" },
        { proto: "UDP", local: "127.0.0.1", port: 5353, state: "", pid: 5108, pname: "svchost.exe" },
      ];
    case "probe_server": {
      const n = args.count || 5;
      const samples = [];
      for (let i = 0; i < n; i++) samples.push(Math.random() < 0.9 ? 180 + Math.floor(Math.random() * 260) : null);
      const ok = samples.filter(Boolean);
      return {
        host: args.host, port: args.port, samples,
        success: ok.length, failed: n - ok.length,
        min_ms: Math.min(...ok), avg_ms: Math.round(ok.reduce((a, b) => a + b, 0) / ok.length), max_ms: Math.max(...ok),
      };
    }
    default:
      throw new Error(`mock 未实现命令: ${cmd}`);
  }
}
