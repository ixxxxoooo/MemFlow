// @author ixxxxoooo
// 平台识别与按平台显示的文案(快捷键修饰键、标题栏留白等)

export type Platform = "macos" | "windows" | "linux";

/** 根据 User-Agent 判断平台(WKWebView 含 Macintosh,WebView2 含 Windows NT,WebKitGTK 含 Linux) */
export function detectPlatform(ua: string = typeof navigator === "undefined" ? "" : navigator.userAgent): Platform {
  if (/Macintosh|Mac OS X/i.test(ua)) return "macos";
  if (/Windows/i.test(ua)) return "windows";
  return "linux";
}

export const platform: Platform = detectPlatform();
export const isMac = platform === "macos";

/** 把当前平台写到 <html data-platform>,供样式按平台区分(毛玻璃、不透明背景等) */
export function applyPlatformAttr(p: Platform = platform): void {
  document.documentElement.dataset.platform = p;
}

/**
 * 应用内快捷键的展示文案
 *
 * 参数: key 主键(如 "N"、",");p 平台
 * 返回: macOS 为「⌘ N」,其它平台为「Ctrl+N」
 */
export function shortcutLabel(key: string, p: Platform = platform): string {
  const k = key.length === 1 ? key.toUpperCase() : key;
  return p === "macos" ? `⌘ ${k}` : `Ctrl+${k}`;
}
