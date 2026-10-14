// @author ygw
// 右栏溯源/详情抽屉:展示原始记录、原图、切片(高亮被引用的那一段)、AI 元数据;可编辑、重刷、删除

import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { confirmDialog } from "../../lib/confirm";
import { fmtDate, fmtTime } from "../../lib/format";
import { useApp } from "../../lib/store";
import type { ItemDetail } from "../../lib/types";
import { CloseIcon, EditIcon, RefreshIcon, TrashIcon } from "./icons";

/** 根据附件相对路径生成可显示的 URL */
export function useAttachmentUrl(rel: string | null | undefined): string | null {
  const dataDir = useApp((s) => s.status?.dataDir);
  if (!rel || !dataDir) return null;
  return convertFileSrc(`${dataDir}/attachments/${rel}`);
}

const STATUS_TEXT: Record<string, string> = {
  pending: "等待处理",
  processing: "处理中",
  done: "已完成",
  archived_only: "仅归档(未启用视觉解析)",
  failed: "处理失败",
};

export function ItemDrawer() {
  const { drawer, closeDrawer, toast, dataVersion, bumpData, refreshStatus } = useApp();
  const [detail, setDetail] = useState<ItemDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const hlRef = useRef<HTMLDivElement>(null);
  const imgUrl = useAttachmentUrl(detail?.item.attachmentPath);

  const load = useCallback(async () => {
    if (!drawer) return;
    setLoading(true);
    try {
      setDetail(await api.getItem(drawer.itemId));
    } catch (e) {
      setDetail(null);
      toast(`读取详情失败:${(e as Error).message}`, "err");
    } finally {
      setLoading(false);
    }
  }, [drawer, toast]);

  useEffect(() => {
    setEditing(false);
    void load();
  }, [load, dataVersion]);

  useEffect(() => {
    hlRef.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [detail, drawer?.chunkId]);

  if (!drawer) return null;
  const it = detail?.item;

  const act = async (fn: () => Promise<unknown>, okMsg: string) => {
    try {
      await fn();
      toast(okMsg, "ok");
      bumpData();
      void refreshStatus();
    } catch (e) {
      toast((e as Error).message, "err");
    }
  };

  const kindLabel = it?.kind === "image" ? "图片" : it?.kind === "document" ? "文档" : "记录";

  return (
    <aside className="pop-in flex h-full w-[340px] shrink-0 flex-col border-l border-[var(--line)] bg-[var(--bg)]">
      <div className="flex items-center justify-between border-b border-[var(--line)] px-4 py-3">
        <div className="min-w-0">
          <div className="text-[11px] text-[var(--fg-dim)]">{kindLabel}详情{it ? ` · ${fmtDate(it.createdAt)} ${fmtTime(it.createdAt)}` : ""}</div>
          <div className="truncate text-sm font-semibold">{it?.title ?? it?.originalName ?? (it ? "闪念记录" : "…")}</div>
        </div>
        <button className="rounded-full p-1.5 hover:bg-[var(--bg-soft)]" onClick={closeDrawer}><CloseIcon size={14} /></button>
      </div>

      <div className="selectable flex-1 space-y-4 overflow-y-auto p-4 text-[13px]">
        {loading && !detail && <div className="text-[var(--fg-dim)]">加载中…</div>}
        {it && (
          <>
            {imgUrl && it.kind === "image" && (
              <img src={imgUrl} alt={it.originalName ?? ""} className="max-h-72 w-full rounded-lg border border-[var(--line)] object-contain" />
            )}
            <div className="flex flex-wrap items-center gap-1.5">
              <span className={`rounded px-1.5 py-0.5 text-[11px] ${it.status === "failed" ? "bg-[var(--err)]/15 text-[var(--err)]" : "bg-[var(--bg-soft)] text-[var(--fg-dim)]"}`}>
                {STATUS_TEXT[it.status] ?? it.status}
              </span>
              {it.category && <span className="rounded bg-[var(--bg-soft)] px-1.5 py-0.5 text-[11px] text-[var(--fg-dim)]">{it.category}</span>}
              {it.tags.map((t) => <span key={t} className="rounded bg-[var(--accent-soft)] px-1.5 py-0.5 text-[11px] text-[var(--accent)]">#{t}</span>)}
            </div>
            {it.error && <div className="rounded-lg bg-[var(--err)]/10 p-2 text-xs text-[var(--err)]">{it.error}</div>}
            {it.summary && (
              <section>
                <div className="mb-1 text-[11px] font-medium text-[var(--fg-dim)]">AI 摘要</div>
                <p className="leading-relaxed">{it.summary}</p>
              </section>
            )}
            {it.entities.length > 0 && (
              <section>
                <div className="mb-1 text-[11px] font-medium text-[var(--fg-dim)]">实体</div>
                <div className="flex flex-wrap gap-1.5">{it.entities.map((e) => <span key={e} className="rounded border border-[var(--line)] px-1.5 py-0.5 text-[11px]">{e}</span>)}</div>
              </section>
            )}

            {it.kind !== "document" && (
              <section>
                <div className="mb-1 flex items-center justify-between text-[11px] font-medium text-[var(--fg-dim)]">
                  <span>{it.kind === "image" ? "图片说明" : "原文"}</span>
                  {!editing && <button className="flex items-center gap-1 hover:text-[var(--fg)]" onClick={() => { setDraft(it.content); setEditing(true); }}><EditIcon size={12} />编辑</button>}
                </div>
                {editing ? (
                  <div className="space-y-2">
                    <textarea className="h-40 w-full resize-y rounded-lg border border-[var(--line)] bg-[var(--bg-soft)] p-2 text-[13px] outline-none focus:border-[var(--accent)]" value={draft} onChange={(e) => setDraft(e.target.value)} />
                    <div className="flex justify-end gap-2">
                      <button className="rounded-md px-3 py-1 text-xs hover:bg-[var(--bg-soft)]" onClick={() => setEditing(false)}>取消</button>
                      <button className="rounded-md bg-[var(--accent)] px-3 py-1 text-xs text-[var(--on-accent)]" onClick={async () => { await act(() => api.updateItemContent(it.id, draft), "已保存,正在重新解析"); setEditing(false); }}>保存并重刷</button>
                    </div>
                  </div>
                ) : (
                  <p className="whitespace-pre-wrap leading-relaxed">{it.content || <span className="text-[var(--fg-faint)]">(无文字说明)</span>}</p>
                )}
              </section>
            )}

            {it.visionText && (
              <section>
                <div className="mb-1 text-[11px] font-medium text-[var(--fg-dim)]">视觉解析</div>
                <p className="whitespace-pre-wrap leading-relaxed">{it.visionText}</p>
              </section>
            )}

            {it.kind === "document" && detail && (
              <section>
                <div className="mb-1 text-[11px] font-medium text-[var(--fg-dim)]">切片({detail.chunks.length})</div>
                <div className="space-y-2">
                  {detail.chunks.map((c) => {
                    const hl = c.id === drawer.chunkId;
                    return (
                      <div key={c.id} ref={hl ? hlRef : undefined} className={`rounded-lg border p-2.5 ${hl ? "border-[var(--accent)] bg-[var(--accent-soft)]" : "border-[var(--line)]"}`}>
                        <div className="mb-1 text-[10px] text-[var(--fg-faint)]">#{c.seq + 1}{c.heading ? ` · ${c.heading}` : ""}</div>
                        <p className="whitespace-pre-wrap leading-relaxed">{c.text}</p>
                      </div>
                    );
                  })}
                </div>
              </section>
            )}

            {it.kind !== "document" && detail && detail.chunks.length > 0 && drawer.chunkId && (
              <div ref={hlRef} className="rounded-lg border border-[var(--accent)] bg-[var(--accent-soft)] p-2.5 text-xs">
                <div className="mb-1 text-[10px] text-[var(--fg-dim)]">被引用的片段</div>
                {detail.chunks.find((c) => c.id === drawer.chunkId)?.text ?? ""}
              </div>
            )}
          </>
        )}
      </div>

      {it && (
        <div className="flex items-center justify-between border-t border-[var(--line)] px-4 py-2.5">
          <button className="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs hover:bg-[var(--bg-soft)]" onClick={() => act(() => api.reindex([it.id]), "已加入重刷队列")}>
            <RefreshIcon size={13} />AI 重新解析
          </button>
          <button
            className="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs text-[var(--err)] hover:bg-[var(--err)]/10"
            onClick={async () => {
              if (!(await confirmDialog("确定删除这条记录吗?此操作不可恢复。"))) return;
              await act(() => api.deleteItems([it.id]), "已删除");
              closeDrawer();
            }}
          >
            <TrashIcon size={13} />删除
          </button>
        </div>
      )}
    </aside>
  );
}
