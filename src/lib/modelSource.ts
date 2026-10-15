// @author ygw
// 向量/视觉模型来源的纯逻辑:来源标识(与后端 Settings::embed_identity 保持一致)

import type { Settings } from "./types";

/** 向量来源标识:本地为模型名,云端为 cloud:档案:模型名;变化后旧向量需要重建 */
export function embedIdentity(s: Pick<Settings, "embedProvider" | "embedModel" | "embedProfile" | "embedCloudModel">): string {
  return s.embedProvider === "openai_compat" ? `cloud:${s.embedProfile}:${s.embedCloudModel.trim()}` : s.embedModel;
}

/** 当前设置里是否还有任何本地 Ollama 模型被需要(文本、向量、启用的视觉) */
export function needsOllama(models: { local: boolean; required: boolean }[]): boolean {
  return models.some((m) => m.local && m.required);
}
