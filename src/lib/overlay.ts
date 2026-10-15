// @author ixxxxoooo
// 全屏浮层计数:有浮层打开时,聊天区暂停自动贴底,避免背后列表跳动

let count = 0;

/** 浮层打开时调用 */
export function pushOverlay(): void {
  count += 1;
}

/** 浮层关闭时调用 */
export function popOverlay(): void {
  count = Math.max(0, count - 1);
}

/** 是否有浮层处于打开状态 */
export function overlayOpen(): boolean {
  return count > 0;
}
