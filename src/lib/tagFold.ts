// @author ygw
// 标签折叠:行内只展示最常用的若干个,其余收进"更多标签"浮层

import type { TagCount } from "./types";

/**
 * 把标签拆成"行内展示"与"折叠"两部分
 *
 * 参数: tags 全部标签(已按数量降序);active 当前已选标签(不重复展示);max 行内最多展示个数
 * 返回: visible 行内标签;rest 折叠到浮层的标签
 */
export function foldTags(tags: TagCount[], active: string | null, max: number): { visible: TagCount[]; rest: TagCount[] } {
  const pool = tags.filter((t) => t.tag !== active);
  return { visible: pool.slice(0, max), rest: pool.slice(max) };
}

/**
 * 在标签中按关键词过滤(忽略大小写)
 *
 * 参数: tags 标签列表;keyword 关键词
 * 返回: 过滤后的标签;关键词为空时原样返回
 */
export function filterTags(tags: TagCount[], keyword: string): TagCount[] {
  const k = keyword.trim().toLowerCase();
  if (!k) return tags;
  return tags.filter((t) => t.tag.toLowerCase().includes(k));
}
