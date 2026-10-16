import { createRouter, createWebHashHistory } from "vue-router";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "dashboard", component: () => import("../views/Dashboard.vue") },
    { path: "/proxies", name: "proxies", component: () => import("../views/Proxies.vue") },
    { path: "/config", name: "config", component: () => import("../views/ConfigView.vue") },
    { path: "/logs", name: "logs", component: () => import("../views/LogsView.vue") },
    { path: "/tools", name: "tools", component: () => import("../views/Tools.vue") },
    { path: "/settings", name: "settings", component: () => import("../views/SettingsView.vue") },
  ],
});
