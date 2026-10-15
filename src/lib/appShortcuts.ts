// @author ygw
// 应用内快捷键:⌘, 打开设置、⌘N 记一笔、⌘1/2/3 切换视图。全局唤起胶囊的快捷键见设置页。

export type ShortcutAction = "settings" | "capture" | "chat" | "timeline" | "docs";

/** 设置页展示用的快捷键清单 */
export const APP_SHORTCUTS: { keys: string; label: string; action: ShortcutAction }[] = [
  { keys: "⌘ ,", label: "打开设置", action: "settings" },
  { keys: "⌘ N", label: "记一笔", action: "capture" },
  { keys: "⌘ 1", label: "AI 问答", action: "chat" },
  { keys: "⌘ 2", label: "记忆时间流", action: "timeline" },
  { keys: "⌘ 3", label: "知识档案库", action: "docs" },
];

type KeyLike = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey" | "defaultPrevented"> & { isComposing?: boolean };

/**
 * 把键盘事件映射为应用内动作
 *
 * 参数: e 键盘事件
 * 返回: 动作;不是应用快捷键、带了 Shift/Alt、输入法组字中,或事件已被别处处理(如设置页正在录制快捷键)时返回 null
 */
export function shortcutAction(e: KeyLike): ShortcutAction | null {
  if (e.defaultPrevented || e.isComposing) return null;
  if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey) return null;
  switch (e.key) {
    case ",": return "settings";
    case "n":
    case "N": return "capture";
    case "1": return "chat";
    case "2": return "timeline";
    case "3": return "docs";
    default: return null;
  }
}
