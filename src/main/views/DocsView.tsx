// @author ygw
// 知识档案库:文档列表、批量导入、选中重刷 / 全库重刷 / 删除、切片状态

import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../../lib/api";
import { confirmDialog } from "../../lib/confirm";
import { fmtDate } from "../../lib/format";
import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import type { Item } from "../../lib/types";
import { DeleteItemButton } from "../components/DeleteItemButton";
import { MoveIcon, RefreshIcon, SearchIcon, SparkleIcon, TrashIcon, UploadIcon } from "../components/icons";
import { MoveToMenu } from "../components/MoveToMenu";
import { StatusChip } from "../components/ItemCard";

export function DocsView() {
  const { dataVersion, docModal, openDocModal, toast, status, refreshStatus, bumpData } = useApp();
  const [items, setItems] = useState<Item[]>([]);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [loading, setLoading] = useState(false);
  const { kbs, current, importTarget, rememberImport, move } = useKb();
  const currentName = current == null ? null : kbs.find((k) => k.id === current)?.name ?? null;

  useEffect(() => {
    const t = setTimeout(() => setDebounced(search), 250);
    return () => clearTimeout(t);
  }, [search]);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const rows = await api.listItems({ kind: "document", search: debounced || undefined, limit: 200, kbId: current ?? undefined });
      setItems(rows);
      setSelected((prev) => new Set([...prev].filter((id) => rows.some((r) => r.id === id))));
    } catch (e) {
      toast(`加载失败:${(e as Error).message}`, "err");
    } finally {
      setLoading(false);
    }
  }, [debounced, current, toast]);

  useEffect(() => { void load(); }, [load, dataVersion]);

  const allSelected = items.length > 0 && selected.size === items.length;
  const ids = useMemo(() => [...selected], [selected]);

  const run = async (fn: () => Promise<unknown>, msg: (r: unknown) => string) => {
    try {
      const r = await fn();
      toast(msg(r), "ok");
      bumpData();
      void refreshStatus();
    } catch (e) {
      toast((e as Error).message, "err");
    }
  };

  const importFiles = async () => {
    const picked = await open({ multiple: true, filters: [{ name: "文档", extensions: ["pdf", "md", "markdown", "txt", "docx"] }] });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    const target = importTarget();
    rememberImport(target);
    const targetName = kbs.find((k) => k.id === target)?.name ?? "默认知识库";
    await run(async () => {
      const r = await api.captureSubmit(null, paths, target);
      if (r.errors.length) toast(r.errors.join(";"), "err");
      return r;
    }, (r) => {
      const x = r as { created: number[]; duplicates: number[] };
      return `已导入 ${x.created.length} 个文档到「${targetName}」${x.duplicates.length ? `,${x.duplicates.length} 个已存在` : ""},后台解析中`;
    });
  };

  const stale = status?.stats.staleChunks ?? 0;

  /** 把记录转移到另一个知识库并汇报结果 */
  const doMove = async (moveIds: number[], kbId: number) => {
    try {
      const r = await move(moveIds, kbId);
      const name = kbs.find((k) => k.id === kbId)?.name ?? "目标知识库";
      const skipped = r.skipped.length ? `,${r.skipped.length} 项因目标库已有相同文件而跳过` : "";
      toast(r.moved.length ? `已转移 ${r.moved.length} 项到「${name}」${skipped}` : `没有可转移的项${skipped}`, r.moved.length ? "ok" : "info");
      setSelected(new Set());
      bumpData();
      void refreshStatus();
    } catch (e) {
      toast((e as Error).message, "err");
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div data-tauri-drag-region className="flex h-[52px] shrink-0 items-center gap-3 px-6">
        <h2 className="text-[15px] font-semibold" data-tauri-drag-region>知识档案库</h2>
        <span className="text-xs text-[var(--fg-faint)]" data-tauri-drag-region>
          {currentName ? `「${currentName}」· ${items.length} 个文档` : `${status?.stats.documents ?? 0} 个文档 · ${status?.stats.chunks ?? 0} 个切片`}
        </span>
        <div className="flex-1" data-tauri-drag-region />
        <div className="flex w-52 items-center gap-2 rounded-lg bg-[var(--bg-soft)] px-2.5 py-1.5">
          <SearchIcon size={14} className="text-[var(--fg-faint)]" />
          <input className="w-full bg-transparent text-[13px] outline-none" placeholder="搜索文档" value={search} onChange={(e) => setSearch(e.target.value)} />
        </div>
        <button className="btn btn-primary" onClick={() => void importFiles()}>
          <UploadIcon size={14} />导入文档
        </button>
      </div>

      <div className="flex items-center gap-2 border-y border-[var(--line)] px-6 py-2 text-xs">
        {selected.size > 0 ? (
          <>
            <span className="text-[var(--fg-dim)]">已选 {selected.size} 项</span>
            <button className="flex items-center gap-1.5 rounded-md bg-[var(--accent-soft)] px-2.5 py-1 text-[var(--accent)]" title="重新分块并向量化,不调用 AI,速度快" onClick={() => run(() => api.reindex(ids), (n) => `已加入 ${n} 项到重新索引队列`)}>
              <RefreshIcon size={12} />重新索引
            </button>
            <button className="flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[var(--fg-dim)] hover:bg-[var(--bg-soft)] hover:text-[var(--fg)]" title="调用文本模型生成摘要、标签与实体,较慢" onClick={() => run(() => api.reindexAi(ids), (n) => `已加入 ${n} 项到 AI 解读队列`)}>
              <SparkleIcon size={12} />AI 解读
            </button>
            <MoveToMenu
              className="flex items-center gap-1.5 rounded-md bg-[var(--accent-soft)] px-2.5 py-1 text-[var(--accent)]"
              title="把选中的文档转移到其他知识库(只改归属,无需重新解析)"
              onPick={(kbId) => void doMove(ids, kbId)}
            >
              <MoveIcon size={12} />转移到…
            </MoveToMenu>
            <button className="flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[var(--err)] hover:bg-[var(--err)]/10" onClick={async () => { if (await confirmDialog(`确定删除选中的 ${selected.size} 个文档吗?`)) { await run(() => api.deleteItems(ids), (n) => `已删除 ${n} 项`); setSelected(new Set()); } }}>
              <TrashIcon size={12} />删除
            </button>
            <button className="rounded-md px-2.5 py-1 text-[var(--fg-dim)] hover:bg-[var(--bg-soft)]" onClick={() => setSelected(new Set())}>取消选择</button>
          </>
        ) : (
          <>
            <span className="text-[var(--fg-dim)]">支持 PDF / Markdown / TXT / DOCX,导入后只做快速索引(不调用 AI),也可直接把文件拖进窗口</span>
            <div className="flex-1" />
            <button
              className="flex items-center gap-1.5 rounded-md px-2.5 py-1 hover:bg-[var(--bg-soft)]"
              onClick={async () => { if (await confirmDialog("全库重刷会重新解析所有内容(笔记与图片会调用 AI,文档只做快速索引),可能耗时较长,确定继续吗?")) await run(() => api.reindex(null), (n) => `已加入 ${n} 项到重刷队列`); }}
            >
              <RefreshIcon size={12} />全库重刷
            </button>
          </>
        )}
      </div>
      {stale > 0 && (
        <div className="bg-[var(--accent-soft)] px-6 py-2 text-xs text-[var(--accent)]">有 {stale} 个切片由其他向量模型生成,需要「全库重刷」后才能被检索到。</div>
      )}

      <div className="flex-1 overflow-y-auto">
        {items.length === 0 && !loading ? (
          <div className="pt-24 text-center text-sm text-[var(--fg-dim)]">
            还没有文档。点击右上角「导入文档」,或把文件拖进来。
          </div>
        ) : (
          <table className="w-full text-[13px]">
            <thead className="sticky top-0 bg-[var(--bg)] text-left text-[11px] text-[var(--fg-faint)]">
              <tr>
                <th className="w-12 py-2 pl-6">
                  <input type="checkbox" checked={allSelected} onChange={() => setSelected(allSelected ? new Set() : new Set(items.map((i) => i.id)))} />
                </th>
                <th className="py-2">名称</th>
                <th className="w-28 py-2">状态</th>
                <th className="w-16 py-2">切片</th>
                <th className="w-28 py-2">导入时间</th>
                <th className="w-20 py-2 pr-4"><span className="sr-only">操作</span></th>
              </tr>
            </thead>
            <tbody>
              {items.map((it) => (
                <tr key={it.id} onClick={() => openDocModal(it.id)} className={`group/row cursor-default border-t border-[var(--line)] hover:bg-[var(--bg-soft)] ${docModal?.itemId === it.id ? "bg-[var(--accent-soft)]" : ""}`}>
                  <td className="py-2.5 pl-6" onClick={(e) => e.stopPropagation()}>
                    <input type="checkbox" checked={selected.has(it.id)} onChange={() => setSelected((p) => { const n = new Set(p); if (n.has(it.id)) n.delete(it.id); else n.add(it.id); return n; })} />
                  </td>
                  <td className="py-2.5 pr-3">
                    <div className="flex items-center gap-1.5 font-medium">
                      {it.title ?? it.originalName}
                      {current == null && kbs.length > 1 && (
                        <span className="rounded bg-[var(--bg-soft)] px-1.5 py-px text-[10.5px] font-normal text-[var(--fg-dim)]">{kbs.find((k) => k.id === it.kbId)?.name}</span>
                      )}
                    </div>
                    {it.summary && <div className="line-clamp-1 text-xs text-[var(--fg-dim)]">{it.summary}</div>}
                    <div className="mt-1 flex flex-wrap gap-1">{it.tags.slice(0, 5).map((t) => <span key={t} className="rounded-full bg-[var(--accent-soft)] px-1.5 text-[10.5px] text-[var(--accent)]">#{t}</span>)}</div>
                  </td>
                  <td className="py-2.5">
                    {it.status === "done" ? <span className="text-[11px] text-[var(--ok)]">{it.metaModel ? "已索引 · AI" : "已索引"}</span> : <StatusChip item={it} />}
                  </td>
                  <td className="py-2.5 text-[var(--fg-dim)]">{it.chunkCount}</td>
                  <td className="py-2.5 text-[var(--fg-dim)]">{fmtDate(it.createdAt)}</td>
                  <td className="py-2 pr-4"><div className="flex items-center gap-1">
                    <MoveToMenu
                      className="rounded p-1 text-[var(--fg-dim)] opacity-0 hover:text-[var(--accent)] group-hover/row:opacity-100"
                      title="移动到其他知识库"
                      disabledIds={[it.kbId]}
                      align="right"
                      onPick={(kbId) => void doMove([it.id], kbId)}
                    >
                      <MoveIcon size={14} />
                    </MoveToMenu>
                    <DeleteItemButton id={it.id} confirmText="确定删除这个文档吗?此操作不可恢复。" revealClass="opacity-0 group-hover/row:opacity-100" />
                  </div></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
