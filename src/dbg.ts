import { invoke } from "@tauri-apps/api/core";

// 崩溃诊断日志（正式功能）：全局未捕获异常 / 未处理 rejection 写本地日志。
// 路径：AppData/com.lele.video-compressor-tauri/vct-debug.log（Rust debug_log 命令）。
// 排查黑屏 / 界面异常时优先看这个文件。
export const dbg = (line: string) => {
  void invoke("debug_log", { line }).catch(() => {});
};

window.addEventListener("error", (e) => {
  dbg(
    `[error] ${e.message} @ ${e.filename}:${e.lineno}:${e.colno}` +
      (e.error?.stack ? ` | ${e.error.stack}` : ""),
  );
});
window.addEventListener("unhandledrejection", (e) => {
  const r = e.reason as { message?: unknown; stack?: unknown } | null;
  dbg(
    `[unhandledrejection] ${String(r?.message ?? r ?? "")}` +
      (r?.stack ? ` | ${r.stack}` : ""),
  );
});
