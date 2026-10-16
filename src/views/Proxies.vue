<script setup>
import { computed, ref } from "vue";
import { useAppStore } from "../stores/app";
import { invoke } from "../lib/bridge";
import { proxyRemoteDesc } from "../lib/format";
import ProxyForm from "../components/ProxyForm.vue";
import Modal from "../components/Modal.vue";
import EmptyState from "../components/EmptyState.vue";

const store = useAppStore();
const formOpen = ref(false);
const editing = ref(null);
const deleting = ref(null);
const busy = ref(false);

const rows = computed(() =>
  (store.config?.proxies || []).map((p) => ({ ...p, live: store.liveFor(p.name) }))
);

function openCreate() {
  deleting.value = null;
  editing.value = null;
  formOpen.value = true;
}

function openEdit(p) {
  deleting.value = null;
  editing.value = p;
  formOpen.value = true;
}

async function onSubmit(entry) {
  // R12：同名创建/重命名冲突先在前端拦一道（后端仍会强制校验）
  const existing = store.config?.proxies || [];
  if (!editing.value && existing.some((p) => p.name === entry.name)) {
    store.toastPush(`已存在同名代理「${entry.name}」，如需修改请使用编辑`, "err");
    return;
  }
  if (editing.value && entry.name !== editing.value.name && existing.some((p) => p.name === entry.name)) {
    store.toastPush(`已存在同名代理「${entry.name}」，重命名冲突`, "err");
    return;
  }
  busy.value = true;
  try {
    const res = await invoke("proxy_upsert", {
      oldName: editing.value ? editing.value.name : null,
      entry,
    });
    store.applySaveResult(res);
    formOpen.value = false;
    store.toastPush(editing.value ? "代理已更新" : `代理「${entry.name}」已创建`, "ok");
    store.toastPush(res.verified ? "frpc verify：语法校验通过" : "已保存（跳过校验）", "info");
    // R15：保存后未应用于运行中的进程时明确提示
    if (res.applied === false) store.toastPush("frpc 正在运行，新配置需重启后生效", "warn");
  } catch (e) {
    store.toastPush(String(e), "err");
  } finally {
    busy.value = false;
  }
}

async function confirmDelete() {
  if (!deleting.value) return;
  busy.value = true;
  try {
    const res = await invoke("proxy_delete", { name: deleting.value.name });
    store.applySaveResult(res);
    store.toastPush(`代理「${deleting.value.name}」已删除`, "ok");
    deleting.value = null;
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
        <div class="page-title">代理</div>
        <div class="page-sub">配置文件中的 [[proxies]] 条目，保存即写入并校验{{ store.configPath ? " · " + store.configPath : "" }}
          <template v-if="store.status.pending_apply"> · <span style="color:var(--warn)">有已保存、重启后生效的修改</span></template>
        </div>
      </div>
      <div class="page-actions">
        <button class="btn btn-primary" @click="openCreate">新建代理</button>
      </div>
    </div>

    <div class="card" v-if="rows.length" style="padding:6px 10px">
      <table class="table selectable">
        <thead>
          <tr>
            <th style="width:26px"></th>
            <th>名称</th><th>类型</th><th>本地服务</th><th>对外访问</th><th>运行状态</th>
            <th style="width:130px"></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in rows" :key="p.name">
            <td><span class="dot" :class="p.live ? (p.live.stale ? 'warn' : p.live.status === 'running' ? 'ok' : p.live.err ? 'err' : 'idle') : 'idle'"></span></td>
            <td class="mono" style="font-weight:600">{{ p.name }}</td>
            <td><span class="badge info">{{ p.ptype }}</span></td>
            <td class="mono">{{ p.plugin ? "插件 · " + p.plugin : `${p.local_ip}:${p.local_port}` }}</td>
            <td class="muted">{{ proxyRemoteDesc(p) }}
              <span v-if="p.use_encryption" class="muted small"> · 加密</span>
              <span v-if="p.use_compression" class="muted small"> · 压缩</span>
            </td>
            <td>
              <span v-if="p.live?.stale" class="badge warn"
                :title="`管理接口暂不可达，这是 ${p.live.observed_at || '最后'} 的观测结果`">未知</span>
              <span v-else-if="p.live?.status === 'running'" class="badge ok">在线</span>
              <span v-else-if="p.live?.err" class="badge err" :title="p.live.err">异常 · {{ p.live.err.slice(0, 40) }}</span>
              <span v-else-if="store.running" class="badge warn">等待</span>
              <span v-else class="badge idle">未运行</span>
            </td>
            <td style="text-align:right;white-space:nowrap">
              <button class="btn btn-sm" @click="openEdit(p)">编辑</button>
              <button class="btn btn-sm btn-danger" style="margin-left:6px" @click="formOpen = false; deleting = p">删除</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="card" v-else>
      <EmptyState text="还没有任何代理——创建第一条隧道，把本地服务暴露到公网">
        <button class="btn btn-primary" @click="openCreate">新建代理</button>
      </EmptyState>
    </div>

    <ProxyForm v-model="formOpen" :entry="editing" @submit="onSubmit" />

    <Modal :title="`删除代理「${deleting?.name ?? ''}」？`" :model-value="!!deleting" @close="deleting = null">
      <div style="font-size:13.5px;line-height:1.8">
        将从配置文件中移除该条目，并按既有约定生成时间戳备份（保留最近 10 份）。<br />
        <span class="muted">运行中的 frpc 需要重启后才会应用新配置。</span>
      </div>
      <template #foot>
        <button class="btn" @click="deleting = null">取消</button>
        <button class="btn btn-danger" :disabled="busy" @click="confirmDelete">确认删除</button>
      </template>
    </Modal>
  </div>
</template>
