// @author ygw
// 知识库范围相关的纯函数:@ 提及解析、范围规整与展示、导入目标选择

import type { Kb } from "./types";

/** 输入框中光标前的一次 @ 提及 */
export interface Mention {
  /** @ 字符在文本中的下标 */
  start: number;
  /** @ 之后已输入的筛选词 */
  query: string;
}

/**
 * 找出光标紧前方正在输入的 @ 提及
 *
 * 参数: text 输入框文本;caret 光标位置
 * 返回: 提及信息;光标前不是「@词」(或 @ 前不是空白/行首,如邮箱)时返回 null
 */
export function mentionAt(text: string, caret: number): Mention | null {
  const before = text.slice(0, caret);
  const m = /(^|\s)@([^\s@]*)$/.exec(before);
  if (!m) return null;
  return { start: before.length - m[2].length - 1, query: m[2] };
}

/**
 * 选中知识库后,把输入框里的「@词」删掉(选择结果以标签展示,不留在文本里)
 *
 * 返回: 新文本与新的光标位置
 */
export function removeMention(text: string, mention: Mention, caret: number): { text: string; caret: number } {
  const head = text.slice(0, mention.start);
  let tail = text.slice(caret);
  // @ 前后各有一个空格时合并,避免留下双空格
  if (head.endsWith(" ") && tail.startsWith(" ")) tail = tail.slice(1);
  return { text: head + tail, caret: head.length };
}

/** 按名称筛选知识库(不区分大小写,空词返回全部) */
export function filterKbs(kbs: Kb[], query: string): Kb[] {
  const q = query.trim().toLowerCase();
  return q ? kbs.filter((k) => k.name.toLowerCase().includes(q)) : kbs;
}

/** 去掉已不存在的知识库 id 并去重 */
export function normalizeScope(ids: number[], kbs: Kb[]): number[] {
  const valid = new Set(kbs.map((k) => k.id));
  return [...new Set(ids)].filter((id) => valid.has(id));
}

/** 切换某个知识库是否在范围内 */
export function toggleScope(ids: number[], id: number): number[] {
  return ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
}

/** 范围的展示文案:空 = 全部 */
export function scopeLabel(ids: number[], kbs: Kb[]): string {
  const names = normalizeScope(ids, kbs).map((id) => kbs.find((k) => k.id === id)?.name ?? "");
  return names.length === 0 ? "全部知识库" : names.join("、");
}

/**
 * 决定导入文件应归入哪个知识库
 *
 * 参数: current 侧栏当前选择(null 表示「全部」);lastUsed 最近一次导入所选;kbs 现有知识库
 * 返回: 优先当前选择,其次最近一次导入所选,最后默认知识库(id=1)
 */
export function importTargetKb(current: number | null, lastUsed: number | null, kbs: Kb[]): number {
  const exists = (id: number | null): id is number => id != null && kbs.some((k) => k.id === id);
  if (exists(current)) return current;
  if (exists(lastUsed)) return lastUsed;
  return kbs[0]?.id ?? 1;
}
