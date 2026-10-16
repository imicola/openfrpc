<script setup>
import { computed, ref, reactive, onMounted, onBeforeUnmount } from "vue";
import { useAppStore } from "../stores/app";
import { invoke, listen } from "../lib/bridge";
import { parsePortSpec, PORT_PRESETS, SERVICE_NAMES } from "../lib/format";

const store = useAppStore();
const tab = ref("scan");

/* ============ 端口扫描 ============ */
const scan = reactive({
  host: "",
  spec: "80, 443, 3306, 7000, 8080",
  presets: new Set(),
  timeout: 800,
  concurrency: 300,
  running: false,
  progress: null,
  results: [],
  showClosed: false,
});

const openCount = computed(() => scan.results.filter((r) => r.open).length);
const shownResults = computed(() => (scan.showClosed ? scan.results : scan.results.filter((r) => r.open)));

function togglePreset(name) {
  if (scan.presets.has(name)) scan.presets.delete(name);
  else scan.presets.add(name);
}

function targetPorts() {
  const list = new Set(parsePortSpec(scan.spec));
  for (const name of scan.presets) {
    for (const p of PORT_PRESETS[name]) list.add(p);
  }
  if (list.size === 0) throw new Error("请先输入端口或选择预设");
  if (list.size > 65536) throw new Error("端口数量超过上限");
  return [...list].sort((a, b) => a - b);
}

// R30：扫描事件带任务标识，只接受本任务进度；组件卸载回收订阅
let unlistenProgress = null;
let currentTask = null;
onMounted(async () => {
  unlistenProgress = await listen("scan:progress", (e) => {
    if (scan.running && e.payload?.task === currentTask) {
      scan.progress = e.payload;
    }
  });
});
onBeforeUnmount(() => {
  unlistenProgress?.();
  unlistenProgress = null;
});

async function startScan() {
  try {
    const ports = targetPorts();
    scan.host = scan.host.trim() || store.serverAddr;
    if (!scan.host) throw new Error("请填写目标主机");
    currentTask = `scan-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
    scan.running = true;
    scan.results = [];
    scan.progress = { done: 0, total: ports.length };
    const out = await invoke("scan_ports", {
      host: scan.host,
      ports,
      timeoutMs: scan.timeout,
      concurrency: scan.concurrency,
      task: currentTask,
    });
    scan.results = out;
    const open = out.filter((r) => r.open).length;
    store.toastPush(`扫描完成：${open} 个端口开放（共 ${out.length}）`, open ? "ok" : "info");  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    scan.running = false;
    scan.progress = null;
    currentTask = null;
  }
}

/* ============ 本地端口 ============ */
const local = reactive({ list: [], q: "", loading: false });
async function refreshLocal() {
  local.loading = true;
  try {
    local.list = await invoke("listeners_list");
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    local.loading = false;
  }
}
refreshLocal();
const localFiltered = computed(() => {
  const q = local.q.trim().toLowerCase();
  if (!q) return local.list;
  return local.list.filter((l) =>
    String(l.port).includes(q) || l.pname.toLowerCase().includes(q) || l.local.includes(q)
  );
});

/* ============ 服务器探测 ============ */
const probe = reactive({ host: "", port: null, count: 10, running: false, result: null });
if (store.serverAddr) {
  probe.host = store.serverAddr;
  probe.port = store.serverPort;
}
async function runProbe() {
  probe.host = probe.host.trim() || store.serverAddr;
  if (!probe.host) {
    store.toastPush("请填写服务器地址", "err");
    return;
  }
  probe.running = true;
  probe.result = null;
  try {
    probe.result = await invoke("probe_server", {
      host: probe.host,
      port: Number(probe.port) || 7000,
      count: Number(probe.count) || 10,
    });
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    probe.running = false;
  }
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div>
        <div class="page-title">工具</div>
        <div class="page-sub">端口扫描 · 本地端口占用 · 服务器连通性</div>
      </div>
    </div>

    <div class="tabs">
      <button class="tab" :class="{ on: tab === 'scan' }" @click="tab = 'scan'">端口扫描</button>
      <button class="tab" :class="{ on: tab === 'local' }" @click="tab = 'local'">本地端口占用</button>
      <button class="tab" :class="{ on: tab === 'probe' }" @click="tab = 'probe'">服务器探测</button>
    </div>

    <!-- 端口扫描 -->
    <div v-if="tab === 'scan'" class="card">
      <div class="form-grid" style="grid-template-columns:1.4fr 2fr auto auto auto;align-items:end">
        <div class="field">
          <label>目标主机</label>
          <input class="input mono" v-model="scan.host" :placeholder="store.serverAddr || 'IP 或域名'" />
        </div>
        <div class="field">
          <label>端口 <span class="desc">支持 80,443,8000-8010 区间</span></label>
          <input class="input mono" v-model="scan.spec" placeholder="80, 443, 8000-8010" />
        </div>
        <div class="field">
          <label>超时 ms</label>
          <input class="input mono" v-model.number="scan.timeout" type="number" min="100" max="10000" style="width:88px" />
        </div>
        <div class="field">
          <label>并发</label>
          <input class="input mono" v-model.number="scan.concurrency" type="number" min="1" max="1024" style="width:80px" />
        </div>
        <button class="btn btn-primary" style="height:34px" :disabled="scan.running" @click="startScan()">
          <span v-if="scan.running" class="spin" style="width:12px;height:12px"></span>
          {{ scan.running ? "扫描中" : "开始扫描" }}
        </button>
      </div>

      <div class="row" style="margin-top:12px;gap:6px;flex-wrap:wrap">
        <button v-for="(ports, name) in PORT_PRESETS" :key="name" class="chip" :class="{ on: scan.presets.has(name) }"
          @click="togglePreset(name)">{{ name }} {{ ports.length }}</button>
      </div>

      <div v-if="scan.progress" class="row" style="margin-top:16px;gap:12px">
        <div class="progress" style="flex:1">
          <div :style="{ width: Math.round((scan.progress.done / scan.progress.total) * 100) + '%' }"></div>
        </div>
        <span class="muted small" style="font-variant-numeric:tabular-nums">{{ scan.progress.done }} / {{ scan.progress.total }}</span>
      </div>

      <template v-if="scan.results.length">
        <div class="row between" style="margin:18px 0 8px">
          <div class="small" style="font-weight:600">
            开放 {{ openCount }} / {{ scan.results.length }} 个端口
          </div>
          <label class="row" style="gap:7px;font-size:12.5px;cursor:pointer">
            <input type="checkbox" v-model="scan.showClosed" /> 显示全部结果
          </label>
        </div>
        <div style="border:1px solid var(--line);border-radius:8px;max-height:380px;overflow:auto">
          <table class="table selectable">
            <thead><tr><th style="width:100px">端口</th><th style="width:70px">状态</th><th style="width:110px">延迟</th><th>常见服务</th></tr></thead>
            <tbody>
              <tr v-for="r in shownResults" :key="r.port">
                <td class="mono" style="font-weight:600">{{ r.port }}</td>
                <td><span :class="r.open ? 'badge ok' : 'badge idle'" style="padding:1px 8px">{{ r.open ? "开放" : "关闭" }}</span></td>
                <td class="mono">{{ r.open ? r.latency_ms + " ms" : "—" }}</td>
                <td class="muted">{{ SERVICE_NAMES[r.port] || "—" }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </template>
    </div>

    <!-- 本地端口 -->
    <div v-if="tab === 'local'" class="card">
      <div class="row between" style="margin-bottom:12px">
        <input class="input" v-model="local.q" placeholder="按端口、进程名、地址过滤…" style="width:300px" />
        <button class="btn" :disabled="local.loading" @click="refreshLocal()">
          <span v-if="local.loading" class="spin" style="width:12px;height:12px"></span> 刷新
        </button>
      </div>
      <div style="border:1px solid var(--line);border-radius:8px;max-height:480px;overflow:auto">
        <table class="table selectable">
          <thead><tr><th style="width:90px">端口</th><th style="width:60px">协议</th><th>监听地址</th><th>进程</th><th style="width:90px">PID</th></tr></thead>
          <tbody>
            <tr v-for="(l, i) in localFiltered" :key="i">
              <td class="mono" style="font-weight:600">{{ l.port }}</td>
              <td><span class="badge idle" style="padding:1px 8px">{{ l.proto }}</span></td>
              <td class="mono">{{ l.local }}</td>
              <td>{{ l.pname || "—" }}</td>
              <td class="mono muted">{{ l.pid || "—" }}</td>
            </tr>
          </tbody>
        </table>
        <div v-if="!localFiltered.length" class="empty" style="padding:30px">
          {{ local.loading ? "正在读取…" : "没有监听中的端口" }}
        </div>
      </div>
      <div class="muted small" style="margin-top:10px">新建代理选本地端口前，先在这里确认端口没被占用。</div>
    </div>

    <!-- 服务器探测 -->
    <div v-if="tab === 'probe'" class="card" style="max-width:640px">
      <div class="form-grid" style="grid-template-columns:1.6fr 0.8fr 0.7fr auto;align-items:end">
        <div class="field">
          <label>服务器地址</label>
          <input class="input mono" v-model="probe.host" placeholder="IP 或域名" />
        </div>
        <div class="field">
          <label>端口</label>
          <input class="input mono" v-model.number="probe.port" type="number" />
        </div>
        <div class="field">
          <label>次数</label>
          <input class="input mono" v-model.number="probe.count" type="number" min="1" max="60" />
        </div>
        <button class="btn btn-primary" :disabled="probe.running" @click="runProbe()">
          <span v-if="probe.running" class="spin" style="width:12px;height:12px"></span>
          {{ probe.running ? "探测中" : "开始" }}
        </button>
      </div>

      <template v-if="probe.result">
        <div class="divider"></div>
        <div class="row" style="gap:28px">
          <div><div class="meta-k muted small">最快</div><div class="big-num">{{ probe.result.min_ms ?? "—" }}<span class="unit">ms</span></div></div>
          <div><div class="meta-k muted small">平均</div><div class="big-num">{{ probe.result.avg_ms ?? "—" }}<span class="unit">ms</span></div></div>
          <div><div class="meta-k muted small">最慢</div><div class="big-num">{{ probe.result.max_ms ?? "—" }}<span class="unit">ms</span></div></div>
          <div><div class="meta-k muted small">成功 / 失败</div><div class="big-num">{{ probe.result.success }}<span class="unit"> / {{ probe.result.failed }}</span></div></div>
        </div>
        <div class="row" style="gap:5px;margin-top:18px;align-items:flex-end">
          <div v-for="(s, i) in probe.result.samples" :key="i" class="probe-bar"
            :style="{ height: s ? Math.max(10, Math.min(56, s / 7)) + 'px' : '10px', background: s ? 'var(--accent)' : '#dbe2e9' }"
            :title="s ? s + ' ms' : '失败'"></div>
        </div>
        <div class="muted small" style="margin-top:12px">直连 TCP 探测，不经过 HTTP 代理链路；若 frpc 配置了 proxyURL，实际链路延迟可能不同。</div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.meta-k { font-size: 12px; color: var(--ink-3); }
.big-num { font-size: 24px; font-weight: 700; font-variant-numeric: tabular-nums; margin-top: 2px; }
.big-num .unit { font-size: 12px; font-weight: 400; color: var(--ink-3); margin-left: 3px; }
.probe-bar { width: 15px; border-radius: 3px; }
</style>
