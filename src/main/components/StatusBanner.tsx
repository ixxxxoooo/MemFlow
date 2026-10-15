// @author ygw
// 顶部引导条:Ollama 未运行 / 缺少模型 / 云端密钥未配置 / 向量模型更换需重刷

import { useState } from "react";
import { api } from "../../lib/api";
import { useApp } from "../../lib/store";
import { PullControls } from "./PullControls";

export function StatusBanner() {
  const { status, refreshStatus, toast, bumpData, setView } = useApp();
  const [starting, setStarting] = useState(false);

  if (!status) return null;

  const startOllama = async () => {
    setStarting(true);
    try {
      await api.ollamaStart();
      toast("Ollama 已启动", "ok");
    } catch (e) {
      toast((e as Error).message, "err");
    } finally {
      setStarting(false);
      void refreshStatus();
    }
  };

  const bar = "flex items-center gap-3 border-b border-[var(--line)] px-5 py-2.5 text-[13px]";
  const primary = "btn btn-primary btn-sm";

  if (!status.ollamaRunning) {
    return (
      <div className={`${bar} bg-[color-mix(in_srgb,var(--err)_10%,transparent)]`}>
        <span>未检测到 Ollama 服务(向量化需要它)。新记录会先安全保存,等 Ollama 启动后自动解析。</span>
        <button className={primary} disabled={starting} onClick={startOllama}>{starting ? "启动中…" : "启动 Ollama"}</button>
      </div>
    );
  }

  // 云端文本模型已选择但没有密钥
  const cloudNoKey = status.models.find((m) => !m.local && m.required && !m.installed);
  if (cloudNoKey) {
    return (
      <div className={`${bar} bg-[color-mix(in_srgb,var(--warn)_12%,transparent)]`}>
        <span>已选择云端模型 {cloudNoKey.name},但尚未配置 API Key,问答与自动打标签暂不可用。</span>
        <button className={primary} onClick={() => setView("settings")}>去设置</button>
      </div>
    );
  }

  const missing = status.models.filter((m) => m.local && m.required && !m.installed);
  if (missing.length > 0) {
    return (
      <div className={`${bar} flex-wrap bg-[color-mix(in_srgb,var(--warn)_12%,transparent)]`}>
        <span>需要下载模型才能使用 AI 功能:</span>
        {missing.map((m) => (
          <span key={m.name} className="flex items-center gap-2">
            <span className="font-medium">{m.role} {m.name}</span>
            <PullControls name={m.name} />
          </span>
        ))}
      </div>
    );
  }

  if (status.stats.staleChunks > 0 && status.stats.items > 0 && !status.queue.running && status.queue.pending === 0) {
    return (
      <div className={`${bar} bg-[var(--accent-soft)]`}>
        <span>有 {status.stats.staleChunks} 个切片由其他向量模型生成,当前检索不会命中它们。建议全库重刷。</span>
        <button
          className={primary}
          onClick={async () => {
            try {
              const n = await api.reindex(null);
              toast(`已加入 ${n} 项到重刷队列`, "ok");
              bumpData();
            } catch (e) {
              toast((e as Error).message, "err");
            }
            void refreshStatus();
          }}
        >
          全库重刷
        </button>
      </div>
    );
  }
  return null;
}
