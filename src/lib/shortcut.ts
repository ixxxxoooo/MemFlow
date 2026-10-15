// @author ixxxxoooo
// 把键盘事件转成 Tauri 全局快捷键字符串(如 CommandOrControl+Shift+K)

import { platform, type Platform } from "./platform";

/** 返回快捷键字符串;只按了修饰键或没有修饰键时返回 null */
export function eventToAccelerator(e: KeyboardEvent): string | null {
  const mods: string[] = [];
  if (e.metaKey || e.ctrlKey) mods.push("CommandOrControl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  const code = e.code;
  let key: string | null = null;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit[0-9]$/.test(code)) key = code.slice(5);
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) key = code;
  else if (code === "Space") key = "Space";
  else if (["Enter", "Tab", "Backspace", "Escape"].includes(code)) key = code;
  if (!key || mods.length === 0) return null;
  return [...mods, key].join("+");
}

/**
 * 友好展示全局快捷键
 *
 * 参数: s 快捷键字符串(如 CommandOrControl+Shift+K);p 平台
 * 返回: macOS 用 ⌘ ⌥ ⇧ 符号;其它平台显示为 Ctrl+Alt+Shift
 */
export function prettyAccelerator(s: string, p: Platform = platform): string {
  if (p !== "macos") {
    return s.replace(/CommandOrControl|Command|Cmd|Control/g, "Ctrl").replace(/Option/g, "Alt");
  }
  return s
    .replace(/CommandOrControl|Command|Cmd/g, "⌘")
    .replace(/Control|Ctrl/g, "⌃")
    .replace(/Alt|Option/g, "⌥")
    .replace(/Shift/g, "⇧")
    .replace(/\+/g, " ");
}
