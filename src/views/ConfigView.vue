<script setup>
import { computed, reactive, ref, watch } from "vue";
import { onBeforeRouteLeave } from "vue-router";
import { useAppStore } from "../stores/app";
import { invoke } from "../lib/bridge";
import { askDialog } from "../lib/bridge";
import Toggle from "../components/Toggle.vue";

const store = useAppStore();
const tab = ref("form");
const busy = ref(false);

// ---- 表单模式 ----
const form = reactive({
  server_addr: "", server_port: 7000, auth_method: "token", auth_token: "",
  transport_protocol: "tcp", transport_proxy_url: "", tls_enabled: false,
  heartbeat_interval: null, heartbeat_timeout: null,
});

watch(() => store.config, (c) => syncFromConfig(c), { immediate: true });

function syncFromConfig(c) {
  const s = c?.server;
  if (!s) return;
  form.server_addr = s.server_addr;
  form.server_port = s.server_port;
  form.auth_method = s.auth_method || "token";
  form.auth_token = s.auth_token;
  form.transport_protocol = s.transport_protocol || "tcp";
  form.transport_proxy_url = s.transport_proxy_url || "";
  // 未显式配置 = frp 0.50+ 的有效缺省「启用」
  form.tls_enabled = s.tls_enabled ?? true;
  form.heartbeat_interval = s.heartbeat_interval ?? null;
  form.heartbeat_timeout = s.heartbeat_timeout ?? null;
}

// 数字字段统一 null/number 比较
const numEq = (a, b) => (a ?? null) === (b ?? null);

// R05：所有实际保存的字段都参与改动检测
const dirty = computed(() => {
  const s = store.config?.server;
  if (!s) return false;
  return (
    form.server_addr !== s.server_addr ||
    Number(form.server_port) !== Number(s.server_port) ||
    form.auth_method !== (s.auth_method || "token") ||
    form.auth_token !== s.auth_token ||
    form.transport_protocol !== (s.transport_protocol || "tcp") ||
    form.transport_proxy_url !== (s.transport_proxy_url || "") ||
    form.tls_enabled !== (s.tls_enabled ?? true) ||
    !numEq(form.heartbeat_interval, s.heartbeat_interval) ||
    !numEq(form.heartbeat_timeout, s.heartbeat_timeout)
  );
});

async function saveForm() {
  busy.value = true;
  try {
    const res = await invoke("config_save_server", {
      fields: {
        server_addr: form.server_addr.trim(),
        server_port: Number(form.server_port) || 7000,
        auth_method: form.auth_method,
        auth_token: form.auth_token,
        transport_protocol: form.transport_protocol,
        transport_proxy_url: form.transport_proxy_url.trim(),
        tls_enabled: form.tls_enabled,
        heartbeat_interval: form.heartbeat_interval ? Number(form.heartbeat_interval) : null,
        heartbeat_timeout: form.heartbeat_timeout ? Number(form.heartbeat_timeout) : null,
      },
    });
    store.applySaveResult(res);
    store.toastPush(res.verified ? "已保存并通过 frpc 校验" : "已保存（跳过校验）", "ok");
    if (res.applied === false) store.toastPush("frpc 正在运行，新配置需重启后生效", "warn");
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    busy.value = false;
  }
}

// ---- 源码模式 ----
const srcText = ref("");
const srcDirty = ref(false);
// R21：进入源码模式时记录内容版本号，保存时回传做外部修改冲突检测；
// 配置被外部重载（rev 变化）且本地无草稿时同步刷新编辑器
let srcLoadedRev = "";
watch(
  () => [tab.value, store.config?.rev],
  ([t]) => {
    if (t === "source" && !srcDirty.value) {
      srcText.value = store.config?.text || "";
      srcLoadedRev = store.config?.rev || "";
    }
  }
);

async function validateSource() {
  busy.value = true;
  try {
    const res = await invoke("config_validate", { text: srcText.value });
    if (res.ok) {
      store.toastPush("语法校验通过", "ok");
    } else {
      store.toastPush(res.output || "校验未通过", "err");
    }
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    busy.value = false;
  }
}

async function saveSource() {
  busy.value = true;
  try {
    const res = await invoke("config_save_text", {
      text: srcText.value,
      validate: true,
      expectedRev: srcLoadedRev || null,
    });
    store.applySaveResult(res);
    srcDirty.value = false;
    srcLoadedRev = res.rev || "";
    store.toastPush(res.verified ? "已保存并通过校验" : "已保存", "ok");
    if (res.applied === false) store.toastPush("frpc 正在运行，新配置需重启后生效", "warn");
    syncFromConfig(store.config);
    loadBackups();
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    busy.value = false;
  }
}

// ---- R20：切换模式 / 离开页面前保护未保存草稿 ----
function hasDraft() {
  return (tab.value === "source" && srcDirty.value) || (tab.value === "form" && dirty.value);
}

async function confirmDiscard() {
  return await askDialog("当前有未保存的修改，离开或切换将丢弃这些修改。确定继续？", "未保存的修改");
}

async function switchTab(t) {
  if (t === tab.value) return;
  if (hasDraft() && !(await confirmDiscard())) return;
  if (tab.value === "source") srcDirty.value = false;
  tab.value = t;
}

onBeforeRouteLeave(async () => {
  if (hasDraft() && !(await confirmDiscard())) return false;
  return true;
});

// ---- 备份 ----
const backups = ref([]);
async function loadBackups() {
  try {
    backups.value = await invoke("config_backups");
  } catch {
    backups.value = [];
  }
}
loadBackups();

async function restore(b) {
  const yes = await askDialog(`恢复备份 ${b.name}？当前配置会先另行备份。`, "恢复备份");
  if (!yes) return;
  busy.value = true;
  try {
    const res = await invoke("config_restore", { path: b.path });
    store.applySaveResult(res);
    syncFromConfig(store.config);
    srcText.value = res.text;
    srcLoadedRev = res.rev || "";
    srcDirty.value = false;
    store.toastPush("备份已恢复", "ok");
    loadBackups();
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div>
        <div class="page-title">配置</div>
        <div class="page-sub">{{ store.configPath || "尚未指定配置文件" }}</div>
      </div>
    </div>

    <div class="tabs">
      <button class="tab" :class="{ on: tab === 'form' }" @click="switchTab('form')">常用设置</button>
      <button class="tab" :class="{ on: tab === 'source' }" @click="switchTab('source')">TOML 源码</button>
    </div>

    <!-- 表单模式 -->
    <div v-if="tab === 'form'" class="card">
      <div class="card-title">服务器连接</div>
      <div class="form-grid">
        <div class="field" style="grid-column: span 2">
          <label>服务器地址 <span class="desc">serverAddr</span></label>
          <input class="input mono" v-model="form.server_addr" placeholder="x.x.x.x 或域名" />
        </div>
        <div class="field">
          <label>端口 <span class="desc">serverPort</span></label>
          <input class="input mono" v-model.number="form.server_port" type="number" min="1" max="65535" />
        </div>
        <div class="field">
          <label>传输协议 <span class="desc">transport.protocol</span></label>
          <select class="select" v-model="form.transport_protocol">
            <option value="tcp">tcp</option>
            <option value="kcp">kcp</option>
            <option value="quic">quic</option>
            <option value="websocket">websocket</option>
            <option value="wss">wss</option>
          </select>
        </div>
        <div class="field" style="grid-column: span 2">
          <label>HTTP 代理 <span class="desc">transport.proxyURL，留空则直连</span></label>
          <input class="input mono" v-model="form.transport_proxy_url" placeholder="http://127.0.0.1:7897" />
        </div>
      </div>

      <div class="divider"></div>
      <div class="card-title">认证</div>
      <div class="form-grid">
        <div class="field">
          <label>认证方式 <span class="desc">auth.method</span></label>
          <select class="select" v-model="form.auth_method">
            <option value="token">token</option>
            <option value="oidc">oidc</option>
          </select>
        </div>
        <div class="field">
          <label>令牌 <span class="desc">auth.token</span></label>
          <input class="input mono" v-model="form.auth_token" placeholder="与 frps 一致" />
        </div>
      </div>

      <div class="divider"></div>
      <div class="card-title">传输选项</div>
      <div class="setting-row">
        <div>
          <div class="lab">启用 TLS <span class="muted small">transport.tls.enable</span></div>
          <div class="sub">加密与 frps 的控制连接，建议公网环境开启</div>
        </div>
        <Toggle :on="form.tls_enabled" @toggle="form.tls_enabled = !form.tls_enabled" />
      </div>
      <div class="form-grid" style="margin-top:14px">
        <div class="field">
          <label>心跳间隔（秒） <span class="desc">留空使用默认 30</span></label>
          <input class="input mono" v-model.number="form.heartbeat_interval" type="number" min="1" />
        </div>
        <div class="field">
          <label>心跳超时（秒） <span class="desc">留空使用默认 90</span></label>
          <input class="input mono" v-model.number="form.heartbeat_timeout" type="number" min="1" />
        </div>
      </div>

      <div class="divider"></div>
      <div class="row between">
        <div class="muted small">保存时自动生成时间戳备份并运行 frpc verify</div>
        <button class="btn btn-primary" :disabled="busy || !dirty" @click="saveForm()">
          {{ dirty ? "保存修改" : "无改动" }}
        </button>
      </div>
    </div>

    <!-- 源码模式 -->
    <div v-if="tab === 'source'" class="card">
      <div class="row between" style="margin-bottom:12px">
        <div class="muted small">直接编辑 frpc.toml 原文；未在此界面定义的字段会原样保留</div>
        <div class="row">
          <button class="btn" :disabled="busy" @click="validateSource()">仅校验</button>
          <button class="btn btn-primary" :disabled="busy" @click="saveSource()">保存</button>
        </div>
      </div>
      <textarea class="textarea mono selectable" v-model="srcText" @input="srcDirty = true"
        spellcheck="false" style="min-height:430px;white-space:pre;overflow:auto"></textarea>
    </div>

    <!-- 备份 -->
    <div class="section-title" style="margin-top:26px">历史备份
      <span class="hint">每次写配置前自动生成，保留最近 10 份</span>
    </div>
    <div class="card" v-if="backups.length" style="padding:6px 10px">
      <table class="table">
        <thead><tr><th>备份文件</th><th>生成时间</th><th style="width:90px"></th></tr></thead>
        <tbody>
          <tr v-for="b in backups" :key="b.path">
            <td class="mono">{{ b.name }}</td>
            <td class="muted">{{ b.mtime }}</td>
            <td style="text-align:right"><button class="btn btn-sm" @click="restore(b)">恢复</button></td>
          </tr>
        </tbody>
      </table>
    </div>
    <div class="card muted" v-else style="padding:16px;font-size:13px">暂无备份文件</div>
  </div>
</template>
