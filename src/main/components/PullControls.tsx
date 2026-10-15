// @author ygw
// 模型下载控件:普通下载(经 Ollama)与直连下载(绕过代理 fake-ip 拦截),带进度与取消

import { api } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import { useApp } from "../../lib/store";

interface Props {
  /** 模型名,如 bge-m3、qwen2.5:7b */
  name: string;
  /** 外部禁用(例如设置尚未保存) */
  disabled?: boolean;
  disabledReason?: string;
}

export function PullControls({ name, disabled, disabledReason }: Props) {
  const pull = useApp((s) => s.pull);
  const pullHint = useApp((s) => s.pullHint);
  const startPull = useApp((s) => s.startPull);

  // 当前模型正在下载:展示进度
  if (pull && pull.name === name) {
    const p = pull.progress;
    const pct = p && p.total ? Math.min(100, (p.completed / p.total) * 100) : 5;
    return (
      <span className="flex items-center gap-2 text-xs text-[var(--fg-dim)]">
        <span className="h-1.5 w-32 overflow-hidden rounded bg-[var(--line)]">
          <span className="block h-full bg-[var(--accent)] transition-all" style={{ width: `${pct}%` }} />
        </span>
        <span className="min-w-[110px]">
          {p && p.total ? `${fmtBytes(p.completed)} / ${fmtBytes(p.total)}` : p?.status || "准备中…"}
        </span>
        <span className="text-[var(--fg-faint)]">{pull.direct ? "直连" : ""}</span>
        <button className="btn btn-ghost btn-sm" onClick={() => void api.cancelPull()}>取消</button>
      </span>
    );
  }

  const busy = !!pull;
  const off = busy || disabled;
  const hinted = pullHint === name;
  return (
    <span className="flex flex-wrap items-center gap-2">
      <button
        className={`btn btn-sm ${hinted ? "btn-secondary" : "btn-primary"}`}
        disabled={off}
        title={disabled ? disabledReason : "通过 Ollama 下载"}
        onClick={() => void startPull(name, false)}
      >
        下载
      </button>
      <button
        className={`btn btn-sm ${hinted ? "btn-primary" : "btn-secondary"}`}
        disabled={off}
        title={disabled ? disabledReason : "应用自己下载并写入 Ollama 模型目录,适用于代理 fake-ip 导致下载失败的情况"}
        onClick={() => void startPull(name, true)}
      >
        直连下载
      </button>
      {hinted && <span className="text-xs text-[var(--warn)]">检测到代理拦截,建议使用直连下载</span>}
    </span>
  );
}
