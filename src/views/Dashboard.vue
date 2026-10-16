<script setup>
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { useAppStore } from "../stores/app";
import { fmtUptime, phaseInfo, proxyRemoteDesc } from "../lib/format";
import { invoke } from "../lib/bridge";

const store = useAppStore();
const router = useRouter();
const probing = ref(false);
const probe = ref(null);

const ph = computed(() => phaseInfo(store.status.phase));
// R11：computed 内实际消费每秒 tick，基于 started_epoch 现算，正常运行也会走表
const uptime = computed(() => {
  void store.uptimeTick;
  const s = store.status;
  if (s.phase === "stopped") return "—";
  if (s.started_epoch) return fmtUptime(Math.max(0, Math.floor(Date.now() / 1000) - s.started_epoch));
  return fmtUptime(s.uptime_secs);
});

// R18：user 前缀映射集中在 store.liveFor
const proxiesWithLive = computed(() => {
  const list = store.config?.proxies || [];
  return list.map((p) => ({ ...p, live: store.liveFor(p.name) }));
});

const onlineCount = computed(() =>
  proxiesWithLive.value.filter((p) => p.live?.status === "running" && !p.live?.stale).length
);

const recentLogs = computed(() => store.logs.slice(-9));

async function doProbe() {
  if (!store.serverAddr) {
    store.toastPush("配置中没有 serverAddr，请先完成配置", "err");
    return;
  }
  probing.value = true;
  probe.value = null;
  try {
    probe.value = await invoke("probe_server", { host: store.serverAddr, port: store.serverPort, count: 5 });
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    probing.value = false;
  }
}

const phaseDesc = computed(() => {
  switch (store.status.phase) {
    case "running": return store.status.connected ? "已登录服务器，隧道正常工作" : "进程运行中，尚未确认登录";
    case "starting": return "正在拉起 frpc 进程…";
    case "reconnecting": return "连接中断，正在自动重试…";
    case "stopping": return "正在停止 frpc…";
    default: return "frpc 未在运行，隧道不可用";
  }
});

// 代理行状态徽章（R17：采样过期显示「未知」而非沿用旧在线态）
function liveBadge(live) {
  if (!live) return store.running ? { cls: "warn", text: "等待" } : { cls: "idle", text: "未运行" };
  if (live.stale) return { cls: "warn", text: "未知" };
  if (live.status === "running") return { cls: "ok", text: "在线" };
  if (live.err) return { cls: "err", text: "异常" };
  return { cls: "idle", text: "等待" };
}
</script>

<template>
  <div class="page">
    <!-- 状态主区：先看状态，再操作 -->
    <div class="card hero">
      <div class="hero-main">
        <div class="row" style="gap:14px">
          <span class="dot" :class="store.status.phase === 'running' && store.status.connected ? 'run' : ph.cls"
            style="width:13px;height:13px"></span>
          <div>
            <div style="font-size:21px;font-weight:700;letter-spacing:0.2px">
              frpc {{ ph.text }}<template v-if="store.status.connected && store.running"> · 已连接服务器</template>
            </div>
            <div class="muted" style="margin-top:2px">{{ phaseDesc }}</div>
            <div v-if="store.status.pending_apply" class="row" style="gap:6px;margin-top:6px">
              <span class="badge warn">配置已保存，重启 frpc 后生效</span>
            </div>
          </div>
        </div>

        <div class="hero-meta">
          <div class="meta-item">
            <div class="meta-k">服务器</div>
            <div class="meta-v mono" :class="{ muted: !store.serverAddr }">{{ store.serverAddr || "未配置" }}<template v-if="store.serverAddr">:{{ store.serverPort }}</template></div>
          </div>
          <div class="meta-item">
            <div class="meta-k">已运行</div>
            <div class="meta-v">{{ store.running ? uptime : "—" }}</div>
          </div>
          <div class="meta-item">
            <div class="meta-k">代理</div>
            <div class="meta-v">{{ onlineCount }}/{{ proxiesWithLive.length }} 在线</div>
          </div>
          <div class="meta-item" v-if="store.status.pid">
            <div class="meta-k">PID</div>
            <div class="meta-v mono">{{ store.status.pid }}</div>
          </div>
        </div>
      </div>

      <div class="hero-actions">
        <button v-if="!store.running" class="btn btn-primary btn-lg" @click="store.startFrpc()">启动 frpc</button>
        <button v-else class="btn btn-danger btn-lg" @click="store.stopFrpc()">停止</button>
        <button class="btn btn-lg" :disabled="!store.running" @click="store.restartFrpc()">重启</button>
        <button class="btn btn-lg" @click="router.push('/logs')">查看日志</button>
      </div>
    </div>

    <!-- 首运行引导：未指定配置文件时 -->
    <div v-if="!store.configPath" class="card" style="margin-top:16px;border-color:#e5d9b8;background:#fdf9ef">
      <div class="row between" style="gap:16px">
        <div class="col" style="gap:2px">
          <div style="font-weight:600;color:var(--warn)">还差一步：指定 frpc 配置文件</div>
          <div class="muted small">已{{ store.detected ? `定位到 frpc v${store.detected.version}` : "检测到未安装 frpc" }}，请在设置中选择 frpc.toml，之后就能从这里启动和管理隧道。</div>
        </div>
        <router-link to="/settings"><button class="btn btn-primary">去设置</button></router-link>
      </div>
    </div>

    <!-- R27：错误信息独立于进程存活展示；停止态提供重试入口 -->
    <div v-if="store.status.last_error" class="card" style="margin-top:16px;border-color:#e8c9c4;background:#fdf6f5">
      <div class="row" style="align-items:flex-start;gap:10px">
        <span class="dot err" style="margin-top:6px"></span>
        <div class="col" style="gap:2px;min-width:0">
          <div style="font-weight:600;color:var(--danger)">{{ store.running ? "连接异常" : "上次错误" }}</div>
          <div class="mono small selectable" style="word-break:break-all">{{ store.status.last_error }}</div>
        </div>
        <button v-if="store.running" class="btn btn-sm" style="margin-left:auto" @click="store.restartFrpc()">立即重连</button>
        <button v-else class="btn btn-sm btn-primary" style="margin-left:auto" @click="store.startFrpc()">重试启动</button>
      </div>
    </div>

    <!-- 代理状态 + 服务器连通性 -->
    <div class="grid-2" style="margin-top:16px">
      <div class="card">
        <div class="card-title">
          代理状态
          <router-link to="/proxies" class="link-btn small">管理代理 →</router-link>
        </div>
        <template v-if="proxiesWithLive.length">
          <table class="table selectable">
            <thead>
              <tr><th>名称</th><th>类型</th><th>本地</th><th>对外</th><th>状态</th></tr>
            </thead>
            <tbody>
              <tr v-for="p in proxiesWithLive" :key="p.name">
                <td class="mono">{{ p.name }}</td>
                <td><span class="badge info">{{ p.ptype }}</span></td>
                <td class="mono">{{ p.local_ip }}:{{ p.local_port }}</td>
                <td class="muted">{{ proxyRemoteDesc(p) }}</td>
                <td>
                  <span :class="'badge ' + liveBadge(p.live).cls"
                    :title="p.live?.stale ? `管理接口暂不可达，这是 ${p.live.observed_at || '最后'} 的观测结果` : p.live?.err || ''">
                    {{ liveBadge(p.live).text }}
                  </span>
                </td>
              </tr>
            </tbody>
          </table>
        </template>
        <div v-else class="empty" style="padding:30px 0">
          配置中还没有代理
          <div class="act"><router-link to="/proxies"><button class="btn btn-sm">去添加</button></router-link></div>
        </div>
      </div>

      <div class="card">
        <div class="card-title">服务器连通性
          <button class="btn btn-sm" :disabled="probing || !store.serverAddr" @click="doProbe()">
            <span v-if="probing" class="spin" style="width:11px;height:11px"></span>
            {{ probing ? "探测中" : "立即探测" }}
          </button>
        </div>
        <template v-if="probe">
          <div class="row" style="gap:24px">
            <div><div class="meta-k">最快</div><div class="probe-v">{{ probe.min_ms ?? "—" }}<span class="unit">ms</span></div></div>
            <div><div class="meta-k">平均</div><div class="probe-v">{{ probe.avg_ms ?? "—" }}<span class="unit">ms</span></div></div>
            <div><div class="meta-k">最慢</div><div class="probe-v">{{ probe.max_ms ?? "—" }}<span class="unit">ms</span></div></div>
            <div><div class="meta-k">成功率</div><div class="probe-v">{{ Math.round((probe.success / (probe.success + probe.failed)) * 100) }}<span class="unit">%</span></div></div>
          </div>
          <div class="row" style="gap:4px;margin-top:14px">
            <div v-for="(s, i) in probe.samples" :key="i" class="probe-bar"
              :style="{ height: s ? Math.max(8, Math.min(44, s / 8)) + 'px' : '8px', background: s ? 'var(--accent)' : '#dbe2e9' }"
              :title="s ? s + ' ms' : '失败'"></div>
          </div>
          <div class="muted small" style="margin-top:10px">直连 TCP 探测 ×{{ probe.samples.length }}，不经过 HTTP 代理链路</div>
        </template>
        <div v-else class="muted small" style="padding:6px 0 2px">
          对 frps 做 5 次 TCP 连接测速，检验网络与端口可达性
        </div>
      </div>
    </div>

    <!-- 运行环境：全宽横排 -->
    <div class="card" style="margin-top:16px">
      <div class="card-title">运行环境</div>
      <div class="env-grid">
        <div class="meta-item">
          <div class="meta-k">frpc 版本</div>
          <div class="meta-v mono">{{ store.detected?.version ? "v" + store.detected.version : "未检测到" }}</div>
        </div>
        <div class="meta-item">
          <div class="meta-k">开机自启</div>
          <div class="meta-v">{{ store.settings?.autostart_app ? "已开启" : "未开启" }}</div>
        </div>
        <div class="meta-item">
          <div class="meta-k">意外退出自动重启</div>
          <div class="meta-v">{{ store.settings?.auto_restart ? "开启" : "关闭" }}</div>
        </div>
        <div class="meta-item" style="min-width:0">
          <div class="meta-k">配置文件</div>
          <div class="meta-v mono small" style="max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" :title="store.configPath">{{ store.configPath || "未设置" }}</div>
        </div>
      </div>
    </div>

    <!-- 最近日志 -->
    <div class="section-title">最近日志
      <span class="hint"><router-link to="/logs" class="link-btn">完整日志 →</router-link></span>
    </div>
    <div class="log-view selectable" style="max-height:240px">
      <div v-if="!recentLogs.length" class="empty" style="padding:24px">暂无日志——启动 frpc 后实时显示</div>
      <div v-for="l in recentLogs" :key="l.id" class="log-line" :class="'lv-' + l.level">
        <span class="ts">{{ l.ts.slice(11) }}</span><span class="lv">{{ l.level }}</span>
        <span class="src">{{ l.src }}</span><span class="msg">{{ l.msg }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.hero { display: flex; align-items: center; gap: 28px; padding: 24px 26px; }
.hero-main { flex: 1; min-width: 0; }
.hero-actions { display: flex; gap: 10px; flex: none; }
.hero-meta { display: flex; gap: 36px; margin-top: 18px; flex-wrap: wrap; }
.meta-k { font-size: 12px; color: var(--ink-3); }
.meta-v { font-size: 14px; font-weight: 600; margin-top: 1px; font-variant-numeric: tabular-nums; }
.probe-v { font-size: 22px; font-weight: 700; font-variant-numeric: tabular-nums; }
.probe-v .unit { font-size: 12px; font-weight: 400; color: var(--ink-3); margin-left: 2px; }
.probe-bar { width: 14px; border-radius: 3px; }
.env-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(190px, 1fr)); gap: 14px 28px; }
@media (max-width: 1080px) {
  .hero { flex-direction: column; align-items: stretch; }
}
</style>
