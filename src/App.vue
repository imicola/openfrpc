<script setup>
import { onMounted, onBeforeUnmount, computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useAppStore } from "./stores/app";
import { phaseInfo } from "./lib/format";

const store = useAppStore();
const route = useRoute();

const nav = [
  { to: "/", label: "总览", icon: "dash" },
  { to: "/proxies", label: "代理", icon: "proxy" },
  { to: "/config", label: "配置", icon: "config" },
  { to: "/logs", label: "日志", icon: "log" },
  { to: "/tools", label: "工具", icon: "tool" },
  { to: "/settings", label: "设置", icon: "gear" },
];

/* ---------- 主题 ---------- */
const systemDark = ref(false);
let mediaQuery = null;
const onMedia = (e) => (systemDark.value = e.matches);

const resolvedTheme = computed(() => {
  const t = store.settings?.theme || "system";
  if (t === "system") return systemDark.value ? "dark" : "light";
  return t;
});

watch(
  resolvedTheme,
  (r) => document.documentElement.setAttribute("data-theme", r),
  { immediate: true }
);

/** 侧栏快捷按钮：在浅/深之间切换；「跟随系统」时切到当前的反面 */
function toggleTheme() {
  const cur = store.settings?.theme || "system";
  const target = cur === "system"
    ? resolvedTheme.value === "dark" ? "light" : "dark"
    : cur === "dark" ? "light" : "dark";
  store.saveSettings({ theme: target });
}

const collapsed = computed(() => !!store.settings?.sidebar_collapsed);
function toggleSidebar() {
  store.saveSettings({ sidebar_collapsed: !collapsed.value });
}

/* ---------- 图标 ---------- */
const IconSun = `<svg width="16" height="16" viewBox="0 0 16 16" fill="none">
  <circle cx="8" cy="8" r="2.6" stroke="currentColor" stroke-width="1.5"/>
  <path d="M8 1.2v1.8M8 13v1.8M1.2 8h1.8M13 8h1.8M3.2 3.2l1.3 1.3M11.5 11.5l1.3 1.3M12.8 3.2l-1.3 1.3M4.5 11.5l-1.3 1.3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
</svg>`;
const IconMoon = `<svg width="16" height="16" viewBox="0 0 16 16" fill="none">
  <path d="M13.2 9.6A5.6 5.6 0 0 1 6.4 2.8a5.6 5.6 0 1 0 6.8 6.8Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>
</svg>`;

onMounted(() => {
  store.boot();
  mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
  systemDark.value = mediaQuery.matches;
  mediaQuery.addEventListener("change", onMedia);
});
onBeforeUnmount(() => mediaQuery?.removeEventListener("change", onMedia));
</script>

<template>
  <div class="shell" v-if="store.ready">
    <aside class="sider" :class="{ collapsed }">
      <div class="sider-brand">
        <!-- OpenFrpc 标识：开口环 + 穿出的箭头（几何与 raster 图标一致：环心20,20 R12 缺口±36°） -->
        <svg width="34" height="34" viewBox="0 0 40 40">
          <rect x="1" y="1" width="38" height="38" rx="9" fill="#182636" stroke="rgba(255,255,255,0.14)" stroke-width="1" />
          <path d="M 29.71 12.95 A 12 12 0 1 0 29.71 27.05" fill="none" stroke="#6FD3EE" stroke-width="3.5" stroke-linecap="round" />
          <rect x="3" y="18" width="26" height="4" rx="1" fill="#fff" />
          <path d="M 28.8 13.4 L 37.2 20 L 28.8 26.6 Z" fill="#fff" />
        </svg>
        <div>
          <div class="t">OpenFrpc</div>
          <div class="s">frp 控制台</div>
        </div>
      </div>

      <nav class="nav">
        <router-link v-for="item in nav" :key="item.to" :to="item.to" class="nav-item"
          :class="{ active: route.path === item.to }" :title="collapsed ? item.label : undefined">
          <svg v-if="item.icon === 'dash'" width="16" height="16" viewBox="0 0 16 16" fill="none">
            <rect x="1.5" y="1.5" width="5.5" height="5.5" rx="1.2" stroke="currentColor" stroke-width="1.5" />
            <rect x="9" y="1.5" width="5.5" height="5.5" rx="1.2" stroke="currentColor" stroke-width="1.5" />
            <rect x="1.5" y="9" width="5.5" height="5.5" rx="1.2" stroke="currentColor" stroke-width="1.5" />
            <rect x="9" y="9" width="5.5" height="5.5" rx="1.2" stroke="currentColor" stroke-width="1.5" />
          </svg>
          <svg v-else-if="item.icon === 'proxy'" width="16" height="16" viewBox="0 0 16 16" fill="none">
            <circle cx="3.5" cy="8" r="2" stroke="currentColor" stroke-width="1.5" />
            <circle cx="12.5" cy="3.5" r="2" stroke="currentColor" stroke-width="1.5" />
            <circle cx="12.5" cy="12.5" r="2" stroke="currentColor" stroke-width="1.5" />
            <path d="M5.4 7.2 L10.6 4.3 M5.4 8.8 L10.6 11.7" stroke="currentColor" stroke-width="1.5" />
          </svg>
          <svg v-else-if="item.icon === 'config'" width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path d="M3.5 1.5 H10 L13 4.5 V14.5 H3.5 Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
            <path d="M6 8 H11 M6 11 H11" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
          <svg v-else-if="item.icon === 'log'" width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path d="M2 3.5 H14 M2 7 H14 M2 10.5 H10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
          <svg v-else-if="item.icon === 'tool'" width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path d="M9.8 3.2 A3.4 3.4 0 1 0 5 8.5 L1.8 11.7 L4.3 14.2 L7.5 11 A3.4 3.4 0 0 0 9.8 3.2 Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
          </svg>
          <svg v-else width="16" height="16" viewBox="0 0 16 16" fill="none">
            <circle cx="8" cy="8" r="2.2" stroke="currentColor" stroke-width="1.5" />
            <path d="M8 1.5 V3.2 M8 12.8 V14.5 M1.5 8 H3.2 M12.8 8 H14.5 M3.4 3.4 L4.6 4.6 M11.4 11.4 L12.6 12.6 M12.6 3.4 L11.4 4.6 M4.6 11.4 L3.4 12.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          </svg>
          <span class="nav-label">{{ item.label }}</span>
        </router-link>
      </nav>

      <div class="sider-tools">
        <button class="tool-btn" @click="toggleTheme" :title="resolvedTheme === 'dark' ? '切到浅色' : '切到深色'">
          <span v-html="resolvedTheme === 'dark' ? IconSun : IconMoon"></span>
          <span class="nav-label">{{ resolvedTheme === "dark" ? "浅色模式" : "深色模式" }}</span>
        </button>
        <button class="tool-btn" @click="toggleSidebar" :title="collapsed ? '展开侧栏' : '收起侧栏'">
          <svg class="chev" width="16" height="16" viewBox="0 0 16 16" fill="none">
            <path d="M10 3.5 L5.5 8 L10 12.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
          <span class="nav-label">收起侧栏</span>
        </button>
      </div>

      <div class="sider-foot" :title="`frpc ${phaseInfo(store.status.phase).text}`">
        <span class="dot" :class="phaseInfo(store.status.phase).cls === 'ok' ? (store.status.connected ? 'ok' : 'warn') : phaseInfo(store.status.phase).cls"></span>
        <span class="foot-label">frpc {{ phaseInfo(store.status.phase).text }}</span>
        <span class="ver" style="margin-left:auto" v-if="store.detected?.version">v{{ store.detected.version }}</span>
      </div>
    </aside>

    <main class="main">
      <router-view v-slot="{ Component }">
        <Transition name="page" mode="out-in">
          <component :is="Component" :key="route.path" />
        </Transition>
      </router-view>
    </main>

    <div class="toast-wrap">
      <div v-for="t in store.toast" :key="t.id" class="toast" :class="t.kind">{{ t.text }}</div>
    </div>
  </div>

  <div v-else-if="store.bootError" style="height:100%;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:12px;color:#7b8694">
    <div style="font-weight:600;color:var(--danger)">初始化失败</div>
    <div class="mono small" style="max-width:520px;word-break:break-all;text-align:center">{{ store.bootError }}</div>
    <button class="btn btn-primary" @click="store.bootError = null; store.boot()">重试</button>
  </div>

  <div v-else style="height:100%;display:flex;align-items:center;justify-content:center;color:#7b8694;gap:10px">
    <span class="spin"></span> 正在初始化…
  </div>
</template>
