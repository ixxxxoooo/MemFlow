// @author ygw
// 顶部引导条:Ollama 未运行 / 缺少模型 / 向量模型更换需重刷

import { useState } from "react";
import { api } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import { useApp } from "../../lib/store";
import type { PullProgress } from "../../lib/types";

export function StatusBanner() {
  const { status, refreshStatus, toast, bumpData } = useApp();
  const [starting, setStarting] = useState(false);
  const [pulling, setPulling] = useState<string | null>(null);
  const [progress, setProgress] = useState<PullProgress | null>(null);

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

  const pull = async (name: string) => {
    setPulling(name);
    setProgress(null);
    try {
      await api.pullModel(name, setProgress);
      toast(`${name} 下载完成`, "ok");
    } catch (e) {
      toast(`下载失败:${(e as Error).message}`, "err");
    } finally {
      setPulling(null);
      void refreshStatus();
    }
  };

  const bar = "flex items-center gap-3 border-b border-[var(--line)] px-5 py-2.5 text-[13px]";
  const primary = "rounded-md bg-[var(--accent)] px-3 py-1 text-xs font-medium text-white disabled:opacity-50";

  if (!status.ollamaRunning) {
    return (
      <div className={`${bar} bg-[color-mix(in_srgb,var(--err)_10%,transparent)]`}>
        <span>未检测到 Ollama 服务。新记录会先安全保存,等 Ollama 启动后自动解析。</span>
        <button className={primary} disabled={starting} onClick={startOllama}>{starting ? "启动中…" : "启动 Ollama"}</button>
      </div>
    );
  }

  const missing = status.models.filter((m) => m.required && !m.installed);
  if (missing.length > 0) {
    return (
      <div className={`${bar} flex-wrap bg-[color-mix(in_srgb,var(--warn)_12%,transparent)]`}>
        <span>需要下载模型才能使用 AI 功能:</span>
        {missing.map((m) => (
          <span key={m.name} className="flex items-center gap-2">
            <span className="font-medium">{m.role} {m.name}</span>
            {pulling === m.name ? (
              <span className="flex items-center gap-2 text-xs text-[var(--fg-dim)]">
                <span className="h-1.5 w-32 overflow-hidden rounded bg-[var(--line)]">
                  <span className="block h-full bg-[var(--accent)] transition-all" style={{ width: `${progress && progress.total ? (progress.completed / progress.total) * 100 : 5}%` }} />
                </span>
                {progress && progress.total ? `${fmtBytes(progress.completed)} / ${fmtBytes(progress.total)}` : progress?.status ?? "准备中…"}
                <button className="underline" onClick={() => void api.cancelPull()}>取消</button>
              </span>
            ) : (
              <button className={primary} disabled={!!pulling} onClick={() => pull(m.name)}>一键下载</button>
            )}
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
