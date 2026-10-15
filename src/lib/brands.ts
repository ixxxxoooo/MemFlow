// @author ygw
// 模型品牌识别:根据模型名 / 云端预设 / 来源挑选图标,识别不出时回退到来源图标

import baichuan from "../assets/brands/baichuan-color.svg";
import claude from "../assets/brands/claude-color.svg";
import deepseek from "../assets/brands/deepseek-color.svg";
import doubao from "../assets/brands/doubao-color.svg";
import gemini from "../assets/brands/gemini-color.svg";
import gemma from "../assets/brands/gemma-color.svg";
import kimi from "../assets/brands/kimi-color.svg";
import meta from "../assets/brands/meta-color.svg";
import microsoft from "../assets/brands/microsoft.svg";
import minimax from "../assets/brands/minimax-color.svg";
import mistral from "../assets/brands/mistral-color.svg";
import nvidia from "../assets/brands/nvidia.svg";
import ollama from "../assets/brands/ollama.svg";
import openai from "../assets/brands/openai.svg";
import qwen from "../assets/brands/qwen-color.svg";
import siliconcloud from "../assets/brands/siliconcloud.svg";
import yi from "../assets/brands/yi-color.svg";
import zhipu from "../assets/brands/zhipu-color.svg";

export type BrandId =
  | "baichuan" | "claude" | "deepseek" | "doubao" | "gemini" | "gemma" | "kimi" | "meta"
  | "microsoft" | "minimax" | "mistral" | "nvidia" | "ollama" | "openai" | "qwen"
  | "siliconcloud" | "yi" | "zhipu";

export const BRAND_ICONS: Record<BrandId, string> = {
  baichuan, claude, deepseek, doubao, gemini, gemma, kimi, meta, microsoft, minimax, mistral,
  nvidia, ollama, openai, qwen, siliconcloud, yi, zhipu,
};

/** 黑色单色图标在深色主题下需要反相显示 */
export const MONO_BRANDS: ReadonlySet<BrandId> = new Set<BrandId>(["microsoft", "nvidia", "ollama", "openai", "siliconcloud"]);

/** 模型名 → 品牌;顺序即优先级(更具体的写在前) */
const MODEL_RULES: [RegExp, BrandId][] = [
  [/deepseek/i, "deepseek"],
  [/qwen|qwq|通义/i, "qwen"],
  [/gemma/i, "gemma"],
  [/gemini/i, "gemini"],
  [/claude/i, "claude"],
  [/llama|meta-/i, "meta"],
  [/mistral|mixtral|ministral|codestral/i, "mistral"],
  [/glm|chatglm|zhipu|智谱/i, "zhipu"],
  [/kimi|moonshot/i, "kimi"],
  [/doubao|豆包|skylark/i, "doubao"],
  [/minimax|abab/i, "minimax"],
  [/(^|[/:\-_ ])yi([\-_:.\d]|$)/i, "yi"],
  [/baichuan|百川/i, "baichuan"],
  [/phi[\-\d]|phi$|microsoft|wizardlm/i, "microsoft"],
  [/nemotron|nvidia/i, "nvidia"],
  [/gpt|chatgpt|(^|[/:\-_ ])o[134]([\-_:.\d]|$)|openai/i, "openai"],
];

/** 云端预设标识 → 品牌 */
const PRESET_BRANDS: Record<string, BrandId> = {
  deepseek: "deepseek",
  qwen: "qwen",
  moonshot: "kimi",
  zhipu: "zhipu",
  siliconflow: "siliconcloud",
  openai: "openai",
};

/**
 * 解析模型对应的品牌
 *
 * 参数: model 模型名;preset 云端预设标识;provider 来源(ollama / openai_compat)
 * 返回: 品牌标识;完全识别不出时,本地返回 ollama,云端返回 null
 */
export function brandOf(model: string | null | undefined, preset?: string | null, provider?: string | null): BrandId | null {
  const name = model ?? "";
  for (const [re, id] of MODEL_RULES) if (re.test(name)) return id;
  if (preset && PRESET_BRANDS[preset]) return PRESET_BRANDS[preset];
  if (provider === "ollama") return "ollama";
  return null;
}
