// @author ygw
// 放大查看的滚轮语义(纯函数):普通滚轮平移,⌘/Ctrl+滚轮(含触控板捏合)缩放

export interface WheelLike {
  deltaX: number;
  deltaY: number;
  ctrlKey: boolean;
  metaKey: boolean;
}

export type WheelIntent =
  | { type: "zoom"; factor: number }
  | { type: "pan"; dx: number; dy: number };

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * 把滚轮事件解释为缩放或平移
 *
 * 参数: e 滚轮事件的必要字段
 * 返回: 缩放时给出倍率(向上滚放大),平移时给出位移像素
 */
export function wheelIntent(e: WheelLike): WheelIntent {
  if (e.ctrlKey || e.metaKey) {
    // 触控板捏合的 deltaY 很小,鼠标滚轮较大,统一限幅后指数换算
    return { type: "zoom", factor: Math.exp(-clamp(e.deltaY, -50, 50) * 0.006) };
  }
  return { type: "pan", dx: -e.deltaX, dy: -e.deltaY };
}
