// @author ygw
// 知识档案库第一级:知识库列表(新建 / 编辑 / 删除 / 进入),顶部可跨库搜索文档

import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import { fmtDate } from "../../lib/format";
import { DEFAULT_KB_ID } from "../../lib/kbScope";
import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import type { Item, Kb } from "../../lib/types";
import { DeleteKbDialog } from "../components/DeleteKbDialog";
import { EditIcon, FolderIcon, PlusIcon, SearchIcon, TrashIcon } from "../components/icons";
import { KbFormDialog } from "../components/KbFormDialog";

export function KbListView() {
  const { kbs } = useKb();
  const { setDocsKb, openDocModal, openDrawer, dataVersion } = useApp();
  const [form, setForm] = useState<Kb | "new" | null>(null);
  const [deleting, setDeleting] = useState<Kb | null>(null);
  const [search, setSearch] = useState("");
  const [hits, setHits] = useState<Item[] | null>(null);

  // 跨库搜索资料(防抖)
  useEffect(() => {
    const q = search.trim();
    if (!q) { setHits(null); return; }
    let alive = true;
    const t = setTimeout(() => {
      api.listItems({ view: "library", search: q, limit: 100 }).then((r) => { if (alive) setHits(r); }).catch(() => { if (alive) setHits([]); });
    }, 250);
    return () => { alive = false; clearTimeout(t); };
  }, [search, dataVersion]);

  const totalDocs = kbs.reduce((n, k) => n + k.documentCount, 0);

  return (
    <div className="flex h-full flex-col">
      <div data-tauri-drag-region className="flex h-[52px] shrink-0 items-center gap-3 px-6">
        <h2 className="text-[15px] font-semibold" data-tauri-drag-region>知识档案库</h2>
        <span className="text-xs text-[var(--fg-faint)]" data-tauri-drag-region>{kbs.length} 个知识库 · {totalDocs} 个文档</span>
        <div className="flex-1" data-tauri-drag-region />
        <div className="flex w-56 items-center gap-2 rounded-lg bg-[var(--bg-soft)] px-2.5 py-1.5">
          <SearchIcon size={14} className="text-[var(--fg-faint)]" />
          <input className="w-full bg-transparent text-[13px] outline-none" placeholder="跨库搜索文档" value={search} onChange={(e) => setSearch(e.target.value)} />
        </div>
        <button className="btn btn-primary" onClick={() => setForm("new")}><PlusIcon size={14} />新建知识库</button>
      </div>

      <div className="flex-1 overflow-y-auto px-6 pb-8 pt-2">
        {hits ? (
          <div data-testid="kb-search-results">
            <div className="mb-2 text-xs text-[var(--fg-faint)]">在所有知识库中找到 {hits.length} 项</div>
            {hits.length === 0 && <div className="pt-16 text-center text-sm text-[var(--fg-dim)]">没有匹配的资料</div>}
            <div className="space-y-1">
              {hits.map((it) => (
                <button key={it.id} className="flex w-full items-center gap-3 rounded-xl border border-[var(--line)] px-3.5 py-2.5 text-left hover:border-[var(--accent)]" onClick={() => (it.kind === "document" ? openDocModal(it.id) : openDrawer(it.id))}>
                  <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium">{it.title ?? it.originalName}</span>
                  <span className="shrink-0 rounded bg-[var(--bg-soft)] px-1.5 py-px text-[10.5px] text-[var(--fg-dim)]">{kbs.find((k) => k.id === it.kbId)?.name}</span>
                  <span className="shrink-0 text-[11px] text-[var(--fg-faint)]">{fmtDate(it.createdAt)}</span>
                </button>
              ))}
            </div>
          </div>
        ) : (
          <div className="grid gap-3" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))" }} data-testid="kb-grid">
            {kbs.map((k) => (
              <div
                key={k.id}
                role="button"
                tabIndex={0}
                data-testid={`kb-card-${k.id}`}
                className="group/card relative cursor-default rounded-2xl border border-[var(--line)] bg-[var(--bg-elev)]/60 p-4 text-left transition-colors hover:border-[var(--accent)]"
                onClick={() => setDocsKb(k.id)}
                onKeyDown={(e) => { if (e.key === "Enter") setDocsKb(k.id); }}
              >
                <div className="flex items-center gap-2">
                  <FolderIcon size={18} className="shrink-0 text-[var(--accent)]" />
                  <span className="min-w-0 flex-1 truncate text-[14.5px] font-semibold">{k.name}</span>
                  {k.id === DEFAULT_KB_ID && <span className="shrink-0 rounded-full bg-[var(--accent-soft)] px-2 py-px text-[10.5px] text-[var(--accent)]">默认</span>}
                </div>
                <div className="mt-2 line-clamp-2 min-h-[34px] text-xs leading-snug text-[var(--fg-dim)]">{k.description || "暂无描述"}</div>
                <div className="mt-3 flex items-center gap-3 text-[11.5px] text-[var(--fg-faint)]">
                  <span>{k.documentCount} 个文档</span>
                  <span>{k.chunkCount} 个切片</span>
                  <span className="ml-auto">{fmtDate(k.createdAt)}</span>
                </div>
                <div className="absolute right-3 top-3 hidden items-center gap-0.5 rounded-lg bg-[var(--bg-elev)] group-hover/card:flex" onClick={(e) => e.stopPropagation()}>
                  <button title="编辑名称与描述" aria-label={`编辑 ${k.name}`} className="rounded p-1.5 text-[var(--fg-dim)] hover:text-[var(--accent)]" onClick={() => setForm(k)}><EditIcon size={13} /></button>
                  {k.id !== DEFAULT_KB_ID && (
                    <button title="删除知识库" aria-label={`删除 ${k.name}`} className="rounded p-1.5 text-[var(--fg-dim)] hover:text-[var(--err)]" onClick={() => setDeleting(k)}><TrashIcon size={13} /></button>
                  )}
                </div>
              </div>
            ))}
            <button className="flex min-h-[132px] flex-col items-center justify-center gap-1.5 rounded-2xl border border-dashed border-[var(--line)] text-[13px] text-[var(--fg-dim)] hover:border-[var(--accent)] hover:text-[var(--accent)]" onClick={() => setForm("new")}>
              <PlusIcon size={18} />新建知识库
            </button>
          </div>
        )}
      </div>

      {form && <KbFormDialog kb={form === "new" ? undefined : form} onClose={() => setForm(null)} />}
      {deleting && <DeleteKbDialog kb={deleting} onClose={() => setDeleting(null)} />}
    </div>
  );
}
