<script setup>
import { computed, nextTick, ref, watch } from "vue";
import { useAppStore } from "../stores/app";
import { invoke, listen, saveFile, askDialog } from "../lib/bridge";

const store = useAppStore();
const levelFilter = ref("ALL");
const search = ref("");
const autoScroll = ref(true);
const paused = ref(false);
const boxRef = ref(null);
const progress = ref(null);

const filtered = computed(() => {
  let list = store.logs;
  if (levelFilter.value !== "ALL") {
    list = list.filter((l) => l.level === levelFilter.value);
  }
  const q = search.value.trim().toLowerCase();
  if (q) {
    list = list.filter((l) => l.msg.toLowerCase().includes(q) || l.src.toLowerCase().includes(q));
  }
  return list;
});

const rendered = computed(() => filtered.value.slice(-1500));

// R29：暂停 = 冻结当前视图快照，恢复后回到实时流
const frozen = ref([]);
const viewLogs = computed(() => (paused.value ? frozen.value : rendered.value));
watch(paused, (p) => {
  if (p) frozen.value = rendered.value.slice();
});

// R29：以最后一条日志 id 触发滚动（缓冲满后长度不再变化也能触发）；
// 用户手动滚离底部时不强行拽回
watch(
  () => (viewLogs.value.length ? viewLogs.value[viewLogs.value.length - 1].id : -1),
  async () => {
    if (autoScroll.value && !paused.value) {
      await nextTick();
      const el = boxRef.value;
      if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 80) {
        el.scrollTop = el.scrollHeight;
      }
    }
  }
);

// 扫描进度条复用此页底部（工具页发起时全局可见；事件带任务标识，容忍旧格式）
listen("scan:progress", (e) => {
  progress.value = e.payload;
  if (e.payload.done >= e.payload.total) {
    setTimeout(() => (progress.value = null), 600);
  }
});

const counts = computed(() => {
  const c = { I: 0, W: 0, E: 0, D: 0 };
  for (const l of store.logs) if (c[l.level] !== undefined) c[l.level]++;
  return c;
});

async function exportLogs() {
  const path = await saveFile({
    title: "导出日志",
    defaultPath: `frpc-logs-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-")}.log`,
    filters: [{ name: "日志", extensions: ["log", "txt"] }],
  });
  if (!path) return;
  try {
    const n = await invoke("export_logs", { path });
    store.toastPush(`已导出 ${n} 行日志`, "ok");
  } catch (e) {
    store.toastPush(String(e), "err");
  }
}

async function clearLogs() {
  const yes = await askDialog("清空当前日志缓冲？（磁盘上的导出文件不受影响）", "清空日志");
  if (!yes) return;
  await invoke("logs_clear");
  store.logs = [];
  frozen.value = [];
  store.toastPush("日志已清空", "ok");
}
</script>

<template>
  <div class="page" style="max-width:1240px">
    <div class="page-head">
      <div>
        <div class="page-title">日志</div>
        <div class="page-sub">frpc 标准输出 · 缓冲 {{ store.logs.length }} 行</div>
      </div>
      <div class="page-actions">
        <button class="btn" :disabled="!store.logs.length" @click="exportLogs()">导出</button>
        <button class="btn btn-danger" :disabled="!store.logs.length" @click="clearLogs()">清空</button>
      </div>
    </div>

    <div class="row" style="margin-bottom:12px;flex-wrap:wrap;gap:10px">
      <div class="row" style="gap:4px">
        <button v-for="lv in ['ALL', 'I', 'W', 'E', 'D']" :key="lv" class="chip" :class="{ on: levelFilter === lv }"
          @click="levelFilter = lv">
          {{ lv === "ALL" ? "全部" : lv }}
          <span v-if="lv !== 'ALL'" class="muted" style="margin-left:3px">{{ counts[lv] }}</span>
        </button>
      </div>
      <input class="input" v-model="search" placeholder="搜索消息或来源…" style="width:260px" />
      <div class="row" style="margin-left:auto;gap:16px">
        <label class="row" style="gap:7px;cursor:pointer;font-size:13px">
          <input type="checkbox" v-model="autoScroll" /> 自动滚动
        </label>
        <label class="row" style="gap:7px;cursor:pointer;font-size:13px">
          <input type="checkbox" v-model="paused" /> 冻结显示
        </label>
      </div>
    </div>

    <div v-if="progress" class="progress" style="margin-bottom:10px">
      <div :style="{ width: Math.round((progress.done / progress.total) * 100) + '%' }"></div>
    </div>

    <div class="log-view selectable" ref="boxRef" style="height:calc(100vh - 300px);min-height:420px">
      <div v-if="!viewLogs.length" class="empty" style="padding-top:80px">
        {{ store.logs.length ? "没有匹配的日志" : "暂无日志——启动 frpc 后实时显示" }}
      </div>
      <div v-if="paused" class="muted small" style="padding:6px 12px;border-bottom:1px dashed var(--line)">
        已冻结当前视图（{{ frozen.length }} 条）；取消勾选后恢复实时滚动
      </div>
      <div v-for="l in viewLogs" :key="l.id" class="log-line" :class="'lv-' + l.level">
        <span class="ts">{{ l.ts.slice(11) }}</span><span class="lv">{{ l.level }}</span>
        <span class="src">{{ l.src }}</span><span class="msg">{{ l.msg }}</span>
      </div>
    </div>
  </div>
</template>
