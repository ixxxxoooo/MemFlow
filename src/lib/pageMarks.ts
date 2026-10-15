/** PDF 解析在正文中插入的页边界标记(私有区字符),展示与编辑前需去掉 */
const PAGE_MARK_RE = /\uE000\n?/g;

/**
 * 去掉 PDF 页边界标记
 * @param text 原始正文
 * @returns 可展示的正文
 */
export function stripPageMarks(text: string): string {
  return text.includes("\uE000") ? text.replace(PAGE_MARK_RE, "") : text;
}

/**
 * 页码标签,如 " · p.3";没有页码时返回空串
 * @param page 页码
 */
export function pageLabel(page?: number | null): string {
  return page ? ` · p.${page}` : "";
}
