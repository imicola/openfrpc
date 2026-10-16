// Tauri 与浏览器的统一桥：无 Tauri 环境回落到 mock，便于纯浏览器视觉验证。
// mock 仅在浏览器模式下动态加载，演示用的定时器不进入桌面端运行路径
export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let invoker, listener, dialogOpen, dialogSave, dialogAsk, openerReveal;

if (isTauri) {
  const core = await import("@tauri-apps/api/core");
  const event = await import("@tauri-apps/api/event");
  const dialog = await import("@tauri-apps/plugin-dialog");
  const opener = await import("@tauri-apps/plugin-opener");
  invoker = core.invoke;
  listener = event.listen;
  dialogOpen = dialog.open;
  dialogSave = dialog.save;
  dialogAsk = dialog.ask;
  openerReveal = opener.revealItemInDir;
} else {
  const mock = await import("./mock");
  invoker = mock.mockInvoke;
  listener = mock.mockListen;
  dialogOpen = async (opts) => {
    await mock.mockInvoke("__noop");
    return opts?.multiple ? [] : null;
  };
  dialogSave = async () => null;
  dialogAsk = async () => true;
  openerReveal = async () => {};
}

export function invoke(cmd, args = {}) {
  return invoker(cmd, args);
}

export function listen(event, handler) {
  return listener(event, handler);
}

/** 选择一个文件；opts: { title, filters } */
export function pickFile(opts) {
  return dialogOpen({ multiple: false, directory: false, ...opts });
}

/** 另存为对话框，返回路径或 null */
export function saveFile(opts) {
  return dialogSave(opts);
}

/** 原生确认对话框，返回 true/false */
export function askDialog(message, title) {
  return dialogAsk(message, { title, kind: "warning" });
}

/** 在资源管理器中显示 */
export function revealPath(path) {
  return openerReveal(path);
}
