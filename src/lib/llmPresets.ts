// @author ixxxxoooo
// 云端模型预设:均为 OpenAI 兼容协议,实际请求地址为 `{baseUrl}/chat/completions`

export interface LlmPreset {
  id: string;
  label: string;
  baseUrl: string;
  model: string;
  /** 申请 API Key 的地址 */
  keyUrl?: string;
}

export const LLM_PRESETS: LlmPreset[] = [
  { id: "deepseek", label: "DeepSeek", baseUrl: "https://api.deepseek.com", model: "deepseek-chat", keyUrl: "https://platform.deepseek.com/api_keys" },
  { id: "qwen", label: "通义千问(阿里云百炼)", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus" },
  { id: "moonshot", label: "Kimi(月之暗面)", baseUrl: "https://api.moonshot.cn/v1", model: "moonshot-v1-8k" },
  { id: "zhipu", label: "智谱 GLM", baseUrl: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash" },
  { id: "siliconflow", label: "硅基流动", baseUrl: "https://api.siliconflow.cn/v1", model: "Qwen/Qwen2.5-7B-Instruct" },
  { id: "openai", label: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  { id: "custom", label: "自定义(OpenAI 兼容)", baseUrl: "", model: "" },
];

/** 按 id 查找预设 */
export function findPreset(id: string): LlmPreset | undefined {
  return LLM_PRESETS.find((p) => p.id === id);
}

/** 判断下载错误是否由代理 fake-ip 引起(Ollama 服务端拒绝重定向到内网地址) */
export function isFakeIpError(msg: string): boolean {
  return /redirect target not allowed|198\.18\./i.test(msg);
}
