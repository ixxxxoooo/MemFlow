// @author ygw
// 文档弹窗:预览(Markdown 渲染)/ 切片列表 / 在线编辑(md 所见即所得,txt 纯文本),保存后自动快速重新索引

import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { confirmDialog } from "../../lib/confirm";
import { editModeOf, renderAsMarkdown } from "../../lib/docKind";
import { fmtDate, fmtTime } from "../../lib/format";
import { isMissingItemError } from "../../lib/itemErrors";
import { useApp } from "../../lib/store";
import type { ItemDetail } from "../../lib/types";
import { CloseIcon, EditIcon, RefreshIcon, SparkleIcon, TrashIcon } from "./icons";
import { Markdown } from "./Markdown";

// 编辑器体积较大,仅在进入编辑时按需加载
const MarkdownEditor = lazy(() => import("./MarkdownEditor"));

type Tab = "preview" | "chunks";

export function DocModal() {
  const { docModal, closeDocModal, itemsDeleted, toast, dataVersion, bumpData, refreshStatus } = useApp();
  const [detail, setDetail] = useState<ItemDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [tab, setTab] = useState<Tab>("preview");
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const hlRef = useRef<HTMLDivElement>(null);

  const it = detail?.item;
  const mode = editModeOf(it?.originalName);
  const dirty = editing && !!it && draft !== it.content;

  const load = useCallback(async () => {
    if (!docModal) return;
    setLoading(true);
    try {
      setDetail(await api.getItem(docModal.itemId));
    } catch (e) {
      setDetail(null);
      const msg = (e as Error).message;
      // 记录已被删除(如后台事件触发的刷新):静默关闭,不弹错
      if (isMissingItemError(msg)) closeDocModal();
      else toast(`读取文档失败:${msg}`, "err");
    } finally {
      setLoading(false);
    }
  }, [docModal, toast, closeDocModal]);

  // 打开另一篇文档时重置界面状态
  useEffect(() => {
    setEditing(false);
    setTab(docModal?.chunkId ? "chunks" : "preview");
    setDetail(null);
  }, [docModal?.itemId, docModal?.chunkId]);

  // 数据变化(如后台重新索引完成)时刷新;编辑中不覆盖草稿
  useEffect(() => { void load(); }, [load, dataVersion]);

  useEffect(() => {
    if (tab === "chunks") hlRef.current?.scrollIntoView({ block: "center" });
  }, [tab, detail, docModal?.chunkId]);

  /** 关闭前检查未保存修改 */
  const requestClose = useCallback(async () => {
    if (dirty && !(await confirmDialog("有未保存的修改,确定放弃并关闭吗?"))) return;
    closeDocModal();
  }, [dirty, closeDocModal]);

  useEffect(() => {
    if (!docModal) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") void requestClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [docModal, requestClose]);

  if (!docModal) return null;

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

  const startEdit = () => {
    if (!it) return;
    setDraft(it.content);
    setEditing(true);
  };

  const save = async () => {
    if (!it) return;
    if (!draft.trim()) { toast("文档内容不能为空", "err"); return; }
    setSaving(true);
    try {
      await api.updateItemContent(it.id, draft);
      toast("已保存,正在重新索引", "ok");
      setEditing(false);
      bumpData();
      void refreshStatus();
    } catch (e) {
      toast((e as Error).message, "err");
    } finally {
      setSaving(false);
    }
  };

  const cancelEdit = async () => {
    if (dirty && !(await confirmDialog("放弃未保存的修改?"))) return;
    setEditing(false);
  };

  const cited = docModal.chunkId ? detail?.chunks.find((c) => c.id === docModal.chunkId) : undefined;
  const aiDone = !!it?.metaModel;
  const btn = "flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-[var(--fg-dim)] hover:bg-[var(--bg-soft)] hover:text-[var(--fg)] disabled:opacity-40";
  const tabCls = (on: boolean) => `rounded-md px-3 py-1 text-xs ${on ? "bg-[var(--accent-soft)] font-medium text-[var(--accent)]" : "text-[var(--fg-dim)] hover:text-[var(--fg)]"}`;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6 backdrop-blur-[2px]" onMouseDown={(e) => { if (e.target === e.currentTarget) void requestClose(); }}>
      <div className="pop-in flex h-full max-h-[860px] w-full max-w-[980px] flex-col overflow-hidden rounded-2xl border border-[var(--line)] bg-[var(--bg)] shadow-2xl">
        <div className="flex items-start justify-between gap-4 border-b border-[var(--line)] px-5 py-3.5">
          <div className="min-w-0">
            <div className="truncate text-[15px] font-semibold">{it?.title ?? it?.originalName ?? "加载中…"}</div>
            {it && (
              <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-[var(--fg-dim)]">
                <span>{fmtDate(it.createdAt)} {fmtTime(it.createdAt)}</span>
                <span>{it.chunkCount} 个切片</span>
                <span className={it.status === "failed" ? "text-[var(--err)]" : it.status === "done" ? "text-[var(--ok)]" : ""}>
                  {it.status === "done" ? (aiDone ? "已索引 · 已 AI 解读" : "已索引") : it.status === "failed" ? "处理失败" : "处理中…"}
                </span>
                {it.tags.slice(0, 6).map((t) => <span key={t} className="rounded-full bg-[var(--accent-soft)] px-2 text-[11px] text-[var(--accent)]">#{t}</span>)}
              </div>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-0.5">
            {editing ? (
              <>
                <button className={btn} onClick={() => void cancelEdit()} disabled={saving}>取消</button>
                <button className="btn btn-primary" disabled={saving || !dirty} onClick={() => void save()}>
                  {saving ? "保存中…" : "保存并重新索引"}
                </button>
              </>
            ) : (
              <>
                {it && mode !== "none" && <button className={btn} onClick={startEdit}><EditIcon size={13} />编辑</button>}
                {it && <button className={btn} title="重新分块并向量化,不调用 AI,速度快" onClick={() => act(() => api.reindex([it.id]), "已加入重新索引队列")}><RefreshIcon size={13} />重新索引</button>}
                {it && <button className={btn} title="调用文本模型生成摘要、标签与实体,较慢" onClick={() => act(() => api.reindexAi([it.id]), "已加入 AI 解读队列")}><SparkleIcon size={13} />AI 解读</button>}
                {it && (
                  <button
                    className="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-[var(--err)] hover:bg-[var(--err)]/10"
                    onClick={async () => {
                      if (!(await confirmDialog("确定删除这个文档吗?此操作不可恢复。"))) return;
                      try {
                        await api.deleteItems([it.id]);
                        // 先关闭弹窗再刷新,避免重新读取已删除的记录
                        itemsDeleted([it.id]);
                        toast("已删除", "ok");
                        void refreshStatus();
                      } catch (e) {
                        toast((e as Error).message, "err");
                      }
                    }}
                  >
                    <TrashIcon size={13} />删除
                  </button>
                )}
              </>
            )}
            <button className="ml-1 rounded-full p-1.5 text-[var(--fg-dim)] hover:bg-[var(--bg-soft)]" onClick={() => void requestClose()} title="关闭 (Esc)"><CloseIcon size={15} /></button>
          </div>
        </div>

        {!editing && (
          <div className="flex items-center gap-1 border-b border-[var(--line)] px-5 py-2">
            <button className={tabCls(tab === "preview")} onClick={() => setTab("preview")}>预览</button>
            <button className={tabCls(tab === "chunks")} onClick={() => setTab("chunks")}>切片{it ? `(${detail?.chunks.length ?? 0})` : ""}</button>
            {it && mode === "none" && <span className="ml-3 text-[11px] text-[var(--fg-faint)]">PDF / DOCX 仅支持预览提取出的文字</span>}
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-y-auto px-8 py-5">
          {loading && !detail && <div className="text-sm text-[var(--fg-dim)]">加载中…</div>}
          {it?.error && <div className="mb-3 rounded-lg bg-[var(--err)]/10 p-2.5 text-xs text-[var(--err)]">{it.error}</div>}
          {it && editing && mode === "wysiwyg" && (
            <Suspense fallback={<div className="text-sm text-[var(--fg-dim)]">编辑器加载中…</div>}>
              <MarkdownEditor key={it.id} value={it.content} onChange={setDraft} />
            </Suspense>
          )}
          {it && editing && mode === "plain" && (
            <textarea
              autoFocus
              className="selectable h-full min-h-[360px] w-full resize-none rounded-lg border border-[var(--line)] bg-[var(--bg-elev)] p-3 text-[14px] leading-relaxed outline-none focus:border-[var(--accent)]"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
          )}
          {it && !editing && tab === "preview" && (
            <div className="mx-auto max-w-[760px]">
              {cited && (
                <div className="mb-4 rounded-lg border border-[var(--accent)] bg-[var(--accent-soft)] p-3 text-xs">
                  <div className="mb-1 text-[11px] text-[var(--fg-dim)]">被引用的片段{cited.heading ? ` · ${cited.heading}` : ""}</div>
                  <div className="selectable whitespace-pre-wrap leading-relaxed">{cited.text}</div>
                </div>
              )}
              {renderAsMarkdown(it.originalName) ? (
                <Markdown text={it.content} />
              ) : (
                <div className="selectable whitespace-pre-wrap text-[14.5px] leading-[1.75]">{it.content}</div>
              )}
            </div>
          )}
          {it && !editing && tab === "chunks" && detail && (
            <div className="mx-auto max-w-[760px] space-y-2.5">
              {detail.chunks.length === 0 && <div className="text-sm text-[var(--fg-dim)]">还没有切片(可能仍在处理中)。</div>}
              {detail.chunks.map((c) => {
                const hl = c.id === docModal.chunkId;
                return (
                  <div key={c.id} ref={hl ? hlRef : undefined} className={`rounded-lg border p-3 ${hl ? "border-[var(--accent)] bg-[var(--accent-soft)]" : "border-[var(--line)]"}`}>
                    <div className="mb-1 text-[11px] text-[var(--fg-faint)]">#{c.seq + 1}{c.heading ? ` · ${c.heading}` : ""}</div>
                    <div className="selectable whitespace-pre-wrap text-[13px] leading-relaxed">{c.text}</div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
