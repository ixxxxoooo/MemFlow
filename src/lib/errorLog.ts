// @author ixxxxoooo
// 全局前端错误落盘:未捕获异常与未处理的 Promise 拒绝都写入后端日志文件

import { api } from "./api";

let installed = false;

/** 安装全局错误监听(重复调用安全);日志写入失败时静默,避免递归报错 */
export function installErrorLog(windowName: string): void {
  if (installed) return;
  installed = true;
  const send = (level: "error" | "warn", msg: string) => {
    void api.logFrontend(level, `[${windowName}] ${msg}`).catch(() => undefined);
  };
  window.addEventListener("error", (e) => {
    send("error", `${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`);
  });
  window.addEventListener("unhandledrejection", (e) => {
    const r = e.reason as { message?: string; stack?: string } | string | undefined;
    send("error", `未处理的 Promise 拒绝: ${typeof r === "string" ? r : r?.stack ?? r?.message ?? String(r)}`);
  });
}
