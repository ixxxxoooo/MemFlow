// @author ygw
// AI 整理相关的纯函数与对话框状态(仅状态,界面在 OrganizeDialog)

import { create } from "zustand";
import type { ChatModelOption, NoteBrief, Settings } from "./types";

/** 对话框:单条整理 / 归并预览 / 碎片建议列表 */
export type OrganizeDialogState =
  | { kind: "single"; itemId: number }
  | { kind: "merge"; ids: number[] }
  | { kind: "suggest" }
  | null;

interface OrganizeStore {
  dialog: OrganizeDialogState;
  openSingle: (itemId: number) => void;
  openMerge: (ids: number[]) => void;
  openSuggest: () => void;
  close: () => void;
}

export const useOrganize = create<OrganizeStore>((set) => ({
  dialog: null,
  openSingle: (itemId) => set({ dialog: { kind: "single", itemId } }),
  openMerge: (ids) => set({ dialog: { kind: "merge", ids } }),
  openSuggest: () => set({ dialog: { kind: "suggest" } }),
  close: () => set({ dialog: null }),
}));

/** 模型引用是否指向云端(本地模型引用以 ollama: 开头) */
export function refIsCloud(ref: string | null | undefined): boolean {
  return !!ref && !ref.startsWith("ollama:");
}

/**
 * 当前整理所用的模型信息:与问答输入框里选中的模型保持一致
 *
 * 参数: sel 用户选择的引用;models 可选模型;settings 设置(用于回退默认)
 * 返回: 引用、展示名、是否云端
 */
export function organizeModel(
  sel: string | null,
  models: ChatModelOption[],
  settings: Settings | undefined,
): { ref: string | null; label: string; cloud: boolean } {
  const picked = sel ? models.find((m) => m.ref === sel) : undefined;
  if (picked) return { ref: picked.ref, label: picked.label, cloud: refIsCloud(picked.ref) };
  if (!settings) return { ref: null, label: "默认模型", cloud: false };
  const cloud = settings.llmProvider !== "ollama";
  return { ref: null, label: cloud ? settings.llmModel : settings.textModel, cloud };
}

/** 归并所选条数是否合法(2–12 条) */
export function mergeCountOk(n: number): boolean {
  return n >= 2 && n <= 12;
}

/** 一条来源的展示名:有标题用标题,否则取预览前若干字 */
export function briefName(b: NoteBrief, max = 28): string {
  const t = b.title?.trim();
  if (t) return t;
  const p = b.preview.replace(/\s+/g, " ").trim();
  return p.length > max ? `${p.slice(0, max)}…` : p || "(空)";
}

/** 相似度显示为百分比 */
export function pct(v: number): string {
  return `${Math.round(v * 100)}%`;
}
