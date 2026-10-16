import { defineStore } from "pinia";
import { invoke, listen } from "../lib/bridge";

export const useAppStore = defineStore("app", {
  state: () => ({
    ready: false,
    bootError: null, // 初始化失败的错误信息（提供重试入口）
    init: null, // app_init 完整返回
    settings: null,
    detected: null, // {path, version}
    config: null, // {path, text, rev, server, proxies, warnings}
    status: { phase: "stopped" },
    logs: [],
    live: [], // frpc 管理接口实时状态
    frpcProcesses: [],
    toast: [],
    // 局部 UI 状态
    uptimeTick: 0,
  }),

  getters: {
    running: (s) => s.status.phase !== "stopped",
    configPath: (s) => s.config?.path || null,
    frpcOk: (s) => !!s.detected && !!s.settings?.frpc_path,
    serverAddr(state) {
      return state.config?.server?.server_addr || "";
    },
    serverPort(state) {
      return state.config?.server?.server_port || 7000;
    },
  },

  actions: {
    toastPush(text, kind = "info") {
      const id = Date.now() + Math.random();
      this.toast.push({ id, text, kind });
      // 防止极端情况下堆积
      if (this.toast.length > 4) this.toast.splice(0, this.toast.length - 4);
      setTimeout(() => {
        this.toast = this.toast.filter((t) => t.id !== id);
      }, 3200);
    },

    async boot() {
      // R28：先订阅事件再取快照；期间已有事件到达时不回填旧快照，日志按 id 去重
      let gotStatus = false;
      let gotLive = false;
      let lastLogId = 0;
      try {
        await listen("frpc:status", (e) => {
          gotStatus = true;
          this.status = e.payload;
        });
        await listen("frpc:log", (e) => {
          if (e.payload.id <= lastLogId) return; // 快照里已有的旧日志
          lastLogId = e.payload.id;
          this.logs.push(e.payload);
          const cap = this.settings?.log_buffer_lines || 5000;
          if (this.logs.length > cap) this.logs.splice(0, this.logs.length - cap);
        });
        await listen("frpc:proxies", (e) => {
          gotLive = true;
          this.live = e.payload;
        });

        this.init = await invoke("app_init");
        this.settings = this.init.settings;
        this.detected = this.init.detected;
        this.config = this.init.config;
        this.logs = this.init.logs || [];
        lastLogId = this.logs.length ? this.logs[this.logs.length - 1].id : 0;
        if (!gotStatus) this.status = this.init.status;
        if (!gotLive) this.live = this.init.live || [];
        this.frpcProcesses = this.init.frpc_processes || [];
        this.ready = true;
        if (this.init.settings_warning) this.toastPush(this.init.settings_warning, "warn");

        // 运行时长每秒跳动
        setInterval(() => this.uptimeTick++, 1000);
      } catch (e) {
        this.bootError = String(e);
      }
    },

    async refreshConfig() {
      this.config = await invoke("config_read");
    },

    async refreshFrpcProcesses() {
      this.frpcProcesses = await invoke("frpc_processes");
    },

    async saveSettings(patch) {
      // R22：乐观更新，失败回滚到旧值并提示
      const prev = this.settings;
      const next = { ...prev, ...patch };
      this.settings = next;
      try {
        await invoke("settings_save", { settings: next });
        return true;
      } catch (e) {
        this.settings = prev;
        this.toastPush("设置保存失败：" + e, "err");
        return false;
      }
    },

    async detectFrpc() {
      this.detected = await invoke("detect_frpc");
      return this.detected;
    },

    async startFrpc() {
      try {
        await invoke("frpc_start");
        this.toastPush("已发出启动指令", "ok");
      } catch (e) {
        this.toastPush(String(e), "err");
      }
    },

    async stopFrpc() {
      try {
        const msg = await invoke("frpc_stop");
        this.toastPush(msg || "正在停止 frpc", "ok");
      } catch (e) {
        this.toastPush(String(e), "err");
      }
    },

    async restartFrpc() {
      try {
        const msg = await invoke("frpc_restart");
        this.toastPush(msg || "frpc 已重启", "ok");
      } catch (e) {
        this.toastPush(String(e), "err");
      }
    },

    /**
     * R18：按配置条目名查运行状态。配置了 user 前缀时，
     * 运行时代理名是 "{user}.{name}"，统一在此映射，页面不再各自猜测
     */
    liveFor(name) {
      const key = this.status.user_prefix ? `${this.status.user_prefix}.${name}` : name;
      const m = new Map(this.live.map((l) => [l.name, l]));
      return m.get(key) || null;
    },

    applySaveResult(res) {
      if (res && res.text !== undefined) {
        this.config = {
          ...(this.config || {}),
          text: res.text,
          rev: res.rev,
          server: res.server,
          proxies: res.proxies,
          warnings: res.warnings || [],
        };
      }
    },
  },
});
