// 展示格式化工具
export function fmtUptime(secs) {
  if (!secs || secs < 0) return "—";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (d > 0) return `${d} 天 ${h} 小时`;
  if (h > 0) return `${h} 小时 ${m} 分`;
  if (m > 0) return `${m} 分 ${s} 秒`;
  return `${s} 秒`;
}

export function clockOf(ts) {
  // "2026-10-16 15:18:31.767" -> "15:18:31"
  if (!ts || ts.length < 19) return ts || "";
  return ts.slice(11, 19);
}

export const PHASE = {
  stopped: { text: "已停止", cls: "idle" },
  starting: { text: "启动中", cls: "warn" },
  running: { text: "运行中", cls: "ok" },
  reconnecting: { text: "重连中", cls: "warn" },
  stopping: { text: "停止中", cls: "warn" },
};

export function phaseInfo(p) {
  return PHASE[p] || PHASE.stopped;
}

// "80,443,8000-8010" -> [80,443,8000...8010]，去重排序。
// R02：解析后立即校验安全整数并裁剪到 1..65535，再进入循环；
// 超大/非安全整数输入直接跳过，绝不允许主线程死循环
export function parsePortSpec(spec, cap = 65536) {
  const out = new Set();
  for (const part of spec.split(/[,，\s]+/).filter(Boolean)) {
    const m = part.match(/^(\d+)(?:-(\d+))?$/);
    if (!m) continue;
    // 超过 15 位的数字不可能落在端口范围，提前放弃
    if (m[1].length > 15 || (m[2] && m[2].length > 15)) continue;
    let a = Number(m[1]);
    let b = m[2] ? Number(m[2]) : a;
    if (!Number.isSafeInteger(a) || !Number.isSafeInteger(b)) continue;
    if (a > b) [a, b] = [b, a];
    a = Math.max(1, a);
    b = Math.min(65535, b);
    for (let p = a; p <= b && out.size <= cap; p++) {
      out.add(p);
    }
  }
  return [...out].sort((x, y) => x - y);
}

export const PORT_PRESETS = {
  "常用 Web": [21, 22, 25, 53, 80, 110, 143, 443, 445, 465, 587, 993, 995, 3000, 5000, 8000, 8080, 8081, 8443, 8888, 9000, 9090],
  "数据库": [1433, 1521, 3306, 5432, 6379, 9200, 11211, 27017],
  "远程与运维": [22, 23, 3389, 5900, 5985, 2375, 2376],
  "frp 常用": [7000, 7400, 7500, 7501],
};

export const SERVICE_NAMES = {
  21: "FTP", 22: "SSH", 23: "Telnet", 25: "SMTP", 53: "DNS", 80: "HTTP", 110: "POP3",
  143: "IMAP", 443: "HTTPS", 445: "SMB", 465: "SMTPS", 587: "Submission", 993: "IMAPS",
  995: "POP3S", 1433: "MSSQL", 1521: "Oracle", 2049: "NFS", 2375: "Docker", 2376: "Docker TLS",
  3000: "Node/Dev", 3306: "MySQL", 3389: "RDP", 5432: "PostgreSQL", 5900: "VNC",
  6379: "Redis", 7000: "frp", 7400: "frp admin", 7500: "frps dashboard", 8000: "HTTP-Alt",
  8080: "HTTP-Alt", 8081: "HTTP-Alt", 8443: "HTTPS-Alt", 8888: "HTTP-Alt", 9000: "Portainer/MinIO",
  9090: "Prometheus", 9200: "Elasticsearch", 11211: "Memcached", 27017: "MongoDB",
};

export function proxyRemoteDesc(p) {
  if (p.ptype === "http" || p.ptype === "https") {
    const parts = [];
    if (p.subdomain) parts.push(`子域 ${p.subdomain}`);
    if (p.custom_domains && p.custom_domains.length) parts.push(p.custom_domains.join(", "));
    return parts.join(" · ") || "—";
  }
  if (p.ptype === "tcp" || p.ptype === "udp") {
    return p.remote_port ? `远端 :${p.remote_port}` : "—";
  }
  // stcp/xtcp/sudp
  return p.secret_key ? `密钥访问` : "—";
}

export function timeAgo(epochSecs) {
  if (!epochSecs) return "";
  const diff = Math.floor(Date.now() / 1000) - epochSecs;
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  return `${Math.floor(diff / 86400)} 天前`;
}
