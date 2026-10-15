// @author ygw
// 文档类型判断:决定弹窗里的渲染方式与是否可编辑

export type DocEditMode = "wysiwyg" | "plain" | "none";

/**
 * 取小写扩展名
 *
 * 参数: name 文件名(可为空)
 * 返回: 不含点的小写扩展名,没有则为空串
 */
export function docExt(name: string | null | undefined): string {
  if (!name) return "";
  const i = name.lastIndexOf(".");
  return i >= 0 ? name.slice(i + 1).toLowerCase() : "";
}

/**
 * 文档的编辑方式:md 所见即所得;txt 纯文本;pdf/docx 仅预览
 *
 * 参数: name 文件名
 * 返回: wysiwyg / plain / none
 */
export function editModeOf(name: string | null | undefined): DocEditMode {
  const e = docExt(name);
  if (e === "md" || e === "markdown") return "wysiwyg";
  if (e === "txt") return "plain";
  return "none";
}

/** 提取结果为 Markdown 的格式(带标题层级与表格);txt/rtf 按原样排版 */
const MD_EXTS = new Set(["md", "markdown", "docx", "pdf", "pptx", "xlsx", "csv", "tsv", "html", "htm", "xhtml", "epub"]);

/**
 * 预览时是否按 Markdown 渲染(除 txt/rtf 外,提取结果都带 # 标题与 Markdown 表格)
 *
 * 参数: name 文件名
 * 返回: 是否按 Markdown 渲染
 */
export function renderAsMarkdown(name: string | null | undefined): boolean {
  return MD_EXTS.has(docExt(name));
}
