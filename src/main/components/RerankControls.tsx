// @author ixxxxoooo
// 本地重排序模型的下载控件:显示安装状态、下载进度与取消

import { api } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import { useApp } from "../../lib/store";

interface Props {
  /** 模型标识 */
  id: string;
  /** 是否已安装 */
  installed: boolean;
  /** 总大小(字节) */
  size: number;
}

export function RerankControls({ id, installed, size }: Props) {
  const pull = useApp((s) => s.pull);
  const startRerankPull = useApp((s) => s.startRerankPull);
  const mine = pull?.name === `rerank:${id}`;

  if (mine) {
    const p = pull.progress;
    const pct = p && p.total ? Math.min(100, (p.completed / p.total) * 100) : 5;
    return (
      <span className="flex items-center gap-2 text-xs text-[var(--fg-dim)]">
        <span className="h-1.5 w-32 overflow-hidden rounded bg-[var(--line)]">
          <span className="block h-full bg-[var(--accent)] transition-all" style={{ width: `${pct}%` }} />
        </span>
        <span className="min-w-[110px]">{p && p.total ? `${fmtBytes(p.completed)} / ${fmtBytes(p.total)}` : p?.status || "准备中…"}</span>
        <button className="btn btn-ghost btn-sm" onClick={() => void api.cancelPull()}>取消</button>
      </span>
    );
  }
  if (installed) return <span className="text-xs text-[var(--ok)]">已安装</span>;
  return (
    <button className="btn btn-primary btn-sm" disabled={!!pull} title={pull ? "已有模型正在下载" : `下载约 ${fmtBytes(size)}`} onClick={() => void startRerankPull(id)}>
      下载({fmtBytes(size)})
    </button>
  );
}
