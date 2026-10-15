// @author ixxxxoooo
// 知识库范围相关的纯函数:问答范围规整与展示、拖拽/导入目标选择

import type { Kb } from "./types";

/** 默认知识库的固定 id(后端迁移时预置) */
export const DEFAULT_KB_ID = 1;

/** 去掉已不存在的知识库 id 并去重 */
export function normalizeScope(ids: number[], kbs: Kb[]): number[] {
  const valid = new Set(kbs.map((k) => k.id));
  return [...new Set(ids)].filter((id) => valid.has(id));
}

/** 切换某个知识库是否在范围内 */
export function toggleScope(ids: number[], id: number): number[] {
  return ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
}

/** 范围的展示文案:空 = 全部;超过两个时折叠成「A、B 等 N 个」 */
export function scopeLabel(ids: number[], kbs: Kb[]): string {
  const names = normalizeScope(ids, kbs).map((id) => kbs.find((k) => k.id === id)?.name ?? "");
  if (names.length === 0) return "全部知识库";
  if (names.length <= 2) return names.join("、");
  return `${names.slice(0, 2).join("、")} 等 ${names.length} 个`;
}

/**
 * 决定拖入主窗口的文件应归入哪个知识库
 *
 * 参数: openedKb 档案库里当前打开的知识库(未打开为 null);inDocs 当前是否在档案库页;kbs 现有知识库
 * 返回: 正在档案库里查看某个库时归入该库,其余情况一律归入默认知识库
 */
export function dropTargetKb(openedKb: number | null, inDocs: boolean, kbs: Kb[]): number {
  if (inDocs && openedKb != null && kbs.some((k) => k.id === openedKb)) return openedKb;
  return kbs.find((k) => k.id === DEFAULT_KB_ID)?.id ?? kbs[0]?.id ?? DEFAULT_KB_ID;
}
