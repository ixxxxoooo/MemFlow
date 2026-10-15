// @author ygw
// 顶部引导条:Ollama 未运行 / 缺少模型 / 云端密钥未配置 / 向量模型更换需重刷

import { needsOllama } from "../../lib/modelSource";
import { useState, type ReactNode } from "react";
import { api } from "../../lib/api";
import { loadDismissed, saveDismissed } from "../../lib/dismissed";
import { useApp } from "../../lib/store";
import { CloseIcon } from "./icons";
import { PullControls } from "./PullControls";

/** 提示条外壳:右侧带关闭按钮;dismiss 为空时不可关闭(如需要持续操作的进度条) */
function Bar({ className, children, onClose }: { className: string; children: ReactNode; onClose?: () => void }) {
  return (
    <div className={className} role="alert">
      {children}
      {onClose && (
        <button
          type="button"
          aria-label="关闭提示"
          title="关闭提示"
          className="ml-auto grid h-6 w-6 shrink-0 place-items-center rounded-md text-[var(--fg-dim)] hover:bg-[var(--bg-soft)] hover:text-[var(--fg)]"
          onClick={onClose}
        >
          <CloseIcon size={14} />
        </button>
      )}
    </div>
  );
}

export function StatusBanner() {
  const { status, refreshStatus, toast, bumpData, setView } = useApp();
  const [starting, setStarting] = useState(false);
  // 已关闭的提示标识(持久化,重启后保持关闭;内容变化后标识不同,会重新提示)
  const [dismissed, setDismissed] = useState<Set<string>>(loadDismissed);
  const close = (id: string) => () => {
    const next = new Set(dismissed).add(id);
    setDismissed(next);
    saveDismissed(next);
  };

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

  if (!status.ollamaRunning && needsOllama(status.models) && !dismissed.has("ollama-down")) {
    return (
      <Bar className={`${bar} bg-[color-mix(in_srgb,var(--err)_10%,transparent)]`} onClose={close("ollama-down")}>
        <span>未检测到 Ollama 服务(向量化需要它)。新记录会先安全保存,等 Ollama 启动后自动解析。</span>
        <button className={primary} disabled={starting} onClick={startOllama}>{starting ? "启动中…" : "启动 Ollama"}</button>
      </Bar>
    );
  }

  // 密钥已保存但钥匙串读取失败(如授权弹窗点了拒绝):提示重新授权,而不是说"未配置"
  const keyErr = status.keyErrors?.[0];
  if (keyErr && !dismissed.has(`key-error:${keyErr.profile}`)) {
    return (
      <Bar className={`${bar} bg-[color-mix(in_srgb,var(--warn)_12%,transparent)]`} onClose={close(`key-error:${keyErr.profile}`)}>
        <span>API Key 已保存,但读取失败:{keyErr.message}。</span>
        <button
          className={primary}
          onClick={async () => {
            try {
              await api.retryKeyAccess(keyErr.profile === "default" ? null : keyErr.profile);
              toast("已读取到 API Key", "ok");
            } catch (e) {
              toast((e as Error).message, "err");
            }
            void refreshStatus();
          }}
        >
          重新授权
        </button>
      </Bar>
    );
  }

  // 云端文本模型已选择但没有密钥
  const cloudNoKey = status.models.find((m) => !m.local && m.required && !m.installed);
  if (cloudNoKey && !dismissed.has(`cloud-no-key:${cloudNoKey.name}`)) {
    return (
      <Bar className={`${bar} bg-[color-mix(in_srgb,var(--warn)_12%,transparent)]`} onClose={close(`cloud-no-key:${cloudNoKey.name}`)}>
        <span>已选择云端模型 {cloudNoKey.name},但尚未配置 API Key,问答与自动打标签暂不可用。</span>
        <button className={primary} onClick={() => setView("settings")}>去设置</button>
      </Bar>
    );
  }

  const missing = status.models.filter((m) => m.local && m.required && !m.installed);
  const missingId = `missing-models:${missing.map((m) => m.name).join(",")}`;
  if (missing.length > 0 && !dismissed.has(missingId)) {
    return (
      <Bar className={`${bar} flex-wrap bg-[color-mix(in_srgb,var(--warn)_12%,transparent)]`} onClose={close(missingId)}>
        <span>需要下载模型才能使用 AI 功能:</span>
        {missing.map((m) => (
          <span key={m.name} className="flex items-center gap-2">
            <span className="font-medium">{m.role} {m.name}</span>
            <PullControls name={m.name} />
          </span>
        ))}
      </Bar>
    );
  }

  const staleId = "stale-chunks";
  if (status.stats.staleChunks > 0 && status.stats.items > 0 && !status.queue.running && status.queue.pending === 0 && !dismissed.has(staleId)) {
    return (
      <Bar className={`${bar} bg-[var(--accent-soft)]`} onClose={close(staleId)}>
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
      </Bar>
    );
  }
  return null;
}
