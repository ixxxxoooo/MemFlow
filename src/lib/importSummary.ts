// @author ygw
// 导入结果的汇总文案:导入 N、重复 M、跳过 K(含原因)

import type { CaptureResult } from "./types";

export interface ImportSummary {
  /** 单行主文案 */
  text: string;
  /** 轻提示类型:有失败为 err,有新增为 ok,否则 info */
  kind: "ok" | "info" | "err";
  /** 需要展开展示的跳过原因明细 */
  detail: string[];
}

/**
 * 把一次导入的结果整理成可展示的汇总
 *
 * 参数: r 后端返回;kbName 目标知识库名称(可空);noun 计数名词,如 "项"/"个文档"
 */
export function summarizeCapture(r: CaptureResult, kbName?: string | null, noun = "项"): ImportSummary {
  const parts: string[] = [];
  // created 里包含文档引用而自动导入的图片:单独计数,不混进"导入 N 项"
  const linked = r.linkedImages ?? 0;
  const main = r.created.length - linked;
  if (main > 0) parts.push(`导入 ${main} ${noun}${kbName ? `到「${kbName}」` : ""}`);
  if (linked > 0) parts.push(`关联图片 ${linked}`);
  if (r.duplicates.length) parts.push(`重复 ${r.duplicates.length}`);
  if (r.skippedCount) parts.push(`跳过 ${r.skippedCount}`);
  if (r.errors.length) parts.push(`失败 ${r.errors.length}`);
  if (parts.length === 0) parts.push("没有可导入的内容");
  const kind = r.errors.length ? "err" : r.created.length ? "ok" : "info";
  return { text: parts.join("、"), kind, detail: r.skipped };
}

/** 展示用:把跳过明细合并成一行(过长时截断) */
export function skipDetailLine(detail: string[], max = 3): string {
  if (detail.length === 0) return "";
  const shown = detail.slice(0, max).join(";");
  return detail.length > max ? `${shown} 等 ${detail.length} 类` : shown;
}
