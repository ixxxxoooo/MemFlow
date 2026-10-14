// @author ygw
// 时间流卡片:记录 / 图片,带 AI 标签与处理状态

import { api } from "../../lib/api";
import { fmtTime } from "../../lib/format";
import { useApp } from "../../lib/store";
import type { Item } from "../../lib/types";
import { AlertIcon, ImageIcon } from "./icons";
import { useAttachmentUrl } from "./ItemDrawer";

export function StatusChip({ item }: { item: Item }) {
  const { toast, refreshStatus, bumpData } = useApp();
  if (item.status === "processing") {
    return <span className="flex items-center gap-1 text-[11px] text-[var(--fg-dim)]"><span className="spin inline-block h-2.5 w-2.5 rounded-full border-[1.5px] border-[var(--accent)] border-t-transparent" />AI 解析中</span>;
  }
  if (item.status === "pending") {
    return <span className="text-[11px] text-[var(--fg-dim)]" title={item.error ?? ""}>{item.error ? "等待中(" + item.error.slice(0, 24) + "…)" : "排队中"}</span>;
  }
  if (item.status === "failed") {
    return (
      <button
        className="flex items-center gap-1 text-[11px] text-[var(--err)] hover:underline"
        title={item.error ?? ""}
        onClick={async (e) => {
          e.stopPropagation();
          try { await api.retryFailed([item.id]); toast("已重新排队", "ok"); bumpData(); void refreshStatus(); } catch (err) { toast((err as Error).message, "err"); }
        }}
      >
        <AlertIcon size={12} />失败,点击重试
      </button>
    );
  }
  if (item.status === "archived_only") return <span className="text-[11px] text-[var(--fg-faint)]">仅归档</span>;
  return null;
}

export function ItemCard({ item, onTag, selected }: { item: Item; onTag: (t: string) => void; selected?: boolean }) {
  const openDrawer = useApp((s) => s.openDrawer);
  const url = useAttachmentUrl(item.attachmentPath);
  const text = item.kind === "image" ? item.content || item.summary || "" : item.content;
  return (
    <div
      onClick={() => openDrawer(item.id)}
      className={`pop-in cursor-default rounded-2xl border bg-[var(--bg-elev)] p-4 transition-colors hover:border-[var(--accent)]/50 ${selected ? "border-[var(--accent)]" : "border-[var(--line)]"}`}
    >
      {item.kind === "image" && (
        <div className="mb-3 overflow-hidden rounded-xl bg-[var(--bg-soft)]">
          {url ? <img src={url} alt={item.originalName ?? ""} loading="lazy" className="max-h-64 w-full object-contain" /> : <div className="flex h-24 items-center justify-center text-[var(--fg-faint)]"><ImageIcon size={24} /></div>}
        </div>
      )}
      {text && <p className="selectable line-clamp-6 whitespace-pre-wrap text-[14px] leading-relaxed">{text}</p>}
      {item.summary && item.kind === "image" && item.content && <p className="mt-1.5 line-clamp-2 text-[12px] text-[var(--fg-dim)]">{item.summary}</p>}
      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        {item.tags.map((t) => (
          <button key={t} className="rounded-full bg-[var(--accent-soft)] px-2 py-0.5 text-[11px] text-[var(--accent)] hover:brightness-110" onClick={(e) => { e.stopPropagation(); onTag(t); }}>#{t}</button>
        ))}
        {item.category && <span className="rounded-full border border-[var(--line)] px-2 py-0.5 text-[11px] text-[var(--fg-dim)]">{item.category}</span>}
        <span className="ml-auto flex items-center gap-2">
          <StatusChip item={item} />
          <span className="text-[11px] text-[var(--fg-faint)]">{fmtTime(item.createdAt)}</span>
        </span>
      </div>
    </div>
  );
}
