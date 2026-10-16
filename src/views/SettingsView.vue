<script setup>
import { computed, reactive, ref, watch } from "vue";
import { useAppStore } from "../stores/app";
import { invoke, isTauri, pickFile, revealPath, askDialog } from "../lib/bridge";
import Toggle from "../components/Toggle.vue";

const store = useAppStore();
const busy = ref(false);

const local = reactive({
  frpc_path: "",
  config_path: "",
});

const pathsDirty = computed(() =>
  local.frpc_path !== (store.settings?.frpc_path || "") ||
  local.config_path !== (store.settings?.config_path || "")
);

// R23：只订阅两个路径字段，且本地有未保存修改时不回填，
// 修改主题/日志缓冲等即时设置不再抹掉路径草稿
watch(
  () => [store.settings?.frpc_path, store.settings?.config_path],
  ([f, c]) => {
    if (!pathsDirty.value) {
      local.frpc_path = f || "";
      local.config_path = c || "";
    }
  },
  { immediate: true }
);

async function savePaths() {
  busy.value = true;
  try {
    await store.saveSettings({ frpc_path: local.frpc_path.trim() || null, config_path: local.config_path.trim() || null });
    const det = await store.detectFrpc();
    await store.refreshConfig();
    store.toastPush(det ? `设置已保存 · frpc v${det.version}` : "设置已保存（未检测到 frpc）", det ? "ok" : "info");
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    busy.value = false;
  }
}

async function autoDetect() {
  const det = await store.detectFrpc();
  if (det) {
    local.frpc_path = det.path;
    await store.saveSettings({ frpc_path: det.path });
    store.toastPush(`已定位 frpc v${det.version}`, "ok");
    if (!local.config_path) await guessConfig(det.path);
  } else {
    store.toastPush("常见位置没有找到 frpc.exe，请手动选择", "err");
  }
}

async function guessConfig(frpcPath) {
  // 同目录找 frpc.toml：暂存路径试探读取，读不到内容则回退
  const dir = frpcPath.replace(/[\\/][^\\/]+$/, "");
  if (dir === frpcPath) return;
  const candidate = dir + "\\frpc.toml";
  const prev = store.settings.config_path;
  await store.saveSettings({ config_path: candidate });
  await store.refreshConfig();
  if (store.config?.text) {
    local.config_path = candidate;
    store.toastPush("已关联同目录的 frpc.toml", "ok");
  } else {
    await store.saveSettings({ config_path: prev });
    await store.refreshConfig();
  }
}

async function pickFrpc() {
  const p = await pickFile({
    title: "选择 frpc.exe",
    filters: [{ name: "frpc", extensions: ["exe"] }],
  });
  if (p) local.frpc_path = p;
}

async function pickConfig() {
  const p = await pickFile({
    title: "选择 frpc 配置文件",
    filters: [{ name: "TOML", extensions: ["toml", "ini"] }],
  });
  if (p) local.config_path = p;
}

// ---- 开关（即时保存） ----
async function toggle(key) {
  const val = !store.settings[key];
  if (key === "autostart_app" && isTauri) {
    // R24：先操作系统开关，成功后按系统真实状态提交设置；失败不落库、可重试
    try {
      const { enable, disable, isEnabled } = await import("@tauri-apps/plugin-autostart");
      if (val) {
        await enable();
      } else {
        await disable();
      }
      const real = await isEnabled();
      await store.saveSettings({ autostart_app: real });
      if (real !== val) {
        store.toastPush(`系统自启动实际状态为「${real ? "已开启" : "未开启"}」，已按实际状态更新`, "warn");
      }
    } catch (e) {
      store.toastPush("系统自启动设置失败: " + e, "err");
    }
    return;
  }
  await store.saveSettings({ [key]: val });
}

// ---- 接管运行中的 frpc（R25） ----
const takeoverBusy = ref(false);
async function killAll() {
  // 进入操作前刷新列表，避免用过期 PID 强杀
  await store.refreshFrpcProcesses();
  const list = store.frpcProcesses;
  if (!list.length) {
    store.toastPush("当前系统没有 frpc 进程在运行", "info");
    return;
  }
  const detail = list.map((p) => `PID ${p.pid} · ${p.exe || p.name}`).join("\n");
  const yes = await askDialog(`确认结束以下 ${list.length} 个 frpc 进程？\n\n${detail}`, "接管确认");
  if (!yes) return;
  takeoverBusy.value = true;
  const failed = [];
  try {
    for (const p of list) {
      try {
        await invoke("frpc_kill_pid", { pid: p.pid });
      } catch (e) {
        failed.push(`PID ${p.pid}: ${e}`);
      }
    }
    await store.refreshFrpcProcesses();
    if (failed.length) {
      store.toastPush(`已处理 ${list.length - failed.length} 个，失败 ${failed.length} 个：${failed[0]}`, "warn");
    } else {
      store.toastPush("已结束全部 frpc 进程", "ok");
    }
  } finally {
    takeoverBusy.value = false;
  }
}

async function openDataDir() {
  if (store.init?.data_dir) revealPath(store.init.data_dir);
}
</script>

<template>
  <div class="page" style="max-width:860px">
    <div class="page-head">
      <div>
        <div class="page-title">设置</div>
        <div class="page-sub">开关即时生效，路径需点「保存」</div>
      </div>
    </div>

    <div class="card">
      <div class="card-title">运行环境</div>
      <div class="col" style="gap:14px">
        <div class="field">
          <label>frpc 程序路径
            <span class="desc" v-if="store.detected">检测到版本 v{{ store.detected.version }}</span>
            <span class="desc" v-else style="color:var(--danger)">未检测到</span>
          </label>
          <div class="row">
            <input class="input mono" style="flex:1" v-model="local.frpc_path" placeholder="D:\apps\frp\...\frpc.exe" />
            <button class="btn" @click="pickFrpc()">浏览</button>
            <button class="btn" @click="autoDetect()">自动检测</button>
          </div>
        </div>
        <div class="field">
          <label>frpc 配置文件 <span class="desc">TOML 格式；编辑与备份都针对此文件</span></label>
          <div class="row">
            <input class="input mono" style="flex:1" v-model="local.config_path" placeholder="D:\apps\frp\...\frpc.toml" />
            <button class="btn" @click="pickConfig()">浏览</button>
            <button class="btn" v-if="local.config_path" @click="revealPath(local.config_path)">所在文件夹</button>
          </div>
        </div>
        <div class="row" style="justify-content:flex-end">
          <button class="btn btn-primary" :disabled="busy || !pathsDirty" @click="savePaths()">
            {{ pathsDirty ? "保存路径" : "无改动" }}
          </button>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-title">外观</div>
      <div class="setting-row">
        <div>
          <div class="lab">主题</div>
          <div class="sub">深浅两套主题分别设计；「跟随系统」随 Windows 设置切换</div>
        </div>
        <div class="row" style="gap:6px">
          <button v-for="opt in [
            { v: 'light', label: '浅色' },
            { v: 'dark', label: '深色' },
            { v: 'system', label: '跟随系统' },
          ]" :key="opt.v" class="chip" :class="{ on: (store.settings?.theme || 'system') === opt.v }"
            @click="store.saveSettings({ theme: opt.v })">{{ opt.label }}</button>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-title">行为</div>
      <div class="setting-row">
        <div>
          <div class="lab">开机自启动 OpenFrpc</div>
          <div class="sub">开机后台启动，不弹窗口（系统托盘可见）</div>
        </div>
        <Toggle :on="!!store.settings?.autostart_app" @toggle="toggle('autostart_app')" />
      </div>
      <div class="setting-row">
        <div>
          <div class="lab">打开 OpenFrpc 时自动运行 frpc</div>
          <div class="sub">应用启动后半秒拉起，配置错误会在日志里说明</div>
        </div>
        <Toggle :on="!!store.settings?.auto_start_frpc" @toggle="toggle('auto_start_frpc')" />
      </div>
      <div class="setting-row">
        <div>
          <div class="lab">frpc 意外退出时自动重启</div>
          <div class="sub">指数退避重试，最多 6 次；稳定运行 1 分钟后计数清零</div>
        </div>
        <Toggle :on="!!store.settings?.auto_restart" @toggle="toggle('auto_restart')" />
      </div>
      <div class="setting-row">
        <div>
          <div class="lab">关闭窗口时最小化到托盘</div>
          <div class="sub">关闭后 OpenFrpc 与 frpc 继续在后台运行</div>
        </div>
        <Toggle :on="!!store.settings?.close_to_tray" @toggle="toggle('close_to_tray')" />
      </div>
      <div class="setting-row">
        <div>
          <div class="lab">退出 OpenFrpc 时停止 frpc</div>
          <div class="sub">关闭则退出后 frpc 继续在系统后台运行</div>
        </div>
        <Toggle :on="!!store.settings?.stop_frpc_on_exit" @toggle="toggle('stop_frpc_on_exit')" />
      </div>
      <div class="setting-row">
        <div>
          <div class="lab">日志缓冲行数</div>
          <div class="sub">内存中保留的日志条数，超出滚动淘汰</div>
        </div>
        <select class="select" style="width:120px" :value="store.settings?.log_buffer_lines"
          @change="store.saveSettings({ log_buffer_lines: Number($event.target.value) })">
          <option :value="1000">1000</option>
          <option :value="5000">5000</option>
          <option :value="20000">20000</option>
        </select>
      </div>
    </div>

    <div class="card">
      <div class="card-title">运行中的 frpc 进程</div>
      <template v-if="store.frpcProcesses.length">
        <table class="table">
          <thead><tr><th>PID</th><th>路径</th><th>启动时间</th></tr></thead>
          <tbody>
            <tr v-for="p in store.frpcProcesses" :key="p.pid">
              <td class="mono">{{ p.pid }}</td>
              <td class="mono small">{{ p.exe || p.name }}</td>
              <td class="muted">{{ p.start }}</td>
            </tr>
          </tbody>
        </table>
        <div class="row between" style="margin-top:12px">
          <span class="muted small">接管：结束这些进程后由 OpenFrpc 拉起并管理</span>
          <button class="btn btn-danger" :disabled="takeoverBusy" @click="killAll()">结束全部 frpc 进程</button>
        </div>
      </template>
      <div v-else class="muted small" style="padding:4px 0">当前系统没有 frpc 进程在运行</div>
    </div>

    <div class="card">
      <div class="card-title">关于</div>
      <div class="col" style="gap:9px;font-size:13px">
        <div class="row between"><span class="muted">OpenFrpc</span><span class="mono">v{{ store.init?.app_version || "—" }}</span></div>
        <div class="row between"><span class="muted">frpc</span><span class="mono">{{ store.detected ? "v" + store.detected.version : "未检测" }}</span></div>
        <div class="row between"><span class="muted">数据目录</span>
          <span class="row" style="gap:8px">
            <span class="mono small">{{ store.init?.data_dir }}</span>
            <button class="btn btn-sm" @click="openDataDir()">打开</button>
          </span>
        </div>
      </div>
    </div>
  </div>
</template>
