// @author ygw
// 原生确认对话框(WKWebView 中 window.confirm 不可靠,使用 dialog 插件)

import { ask } from "@tauri-apps/plugin-dialog";

/** 弹出确认框,用户确认返回 true */
export async function confirmDialog(message: string, title = "MemFlow"): Promise<boolean> {
  try {
    return await ask(message, { title, kind: "warning", okLabel: "确定", cancelLabel: "取消" });
  } catch {
    return window.confirm(message);
  }
}
