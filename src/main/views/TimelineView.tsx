// @author ygw
// 记忆时间流:按天分组的瀑布流,支持搜索与标签过滤

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { fmtDayLabel, groupBy } from "../../lib/format";
import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import type { Item, TagCount } from "../../lib/types";
import { CloseIcon, SearchIcon, SparkleIcon } from "../components/icons";
import { ItemCard } from "../components/ItemCard";
import { filterTags, foldTags } from "../../lib/tagFold";
import { mergeCountOk, useOrganize } from "../../lib/organize";

const PAGE = 40;

export function TimelineView() {
  const { dataVersion, drawer, toast, status, refreshStatus, bumpData } = useApp();
  const [items, setItems] = useState<Item[]>([]);
  const [tags, setTags] = useState<TagCount[]>([]);
  const [tag, setTag] = useState<string | null>(null);
  const [kind, setKind] = useState<"note,image" | "note" | "image">("note,image");
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const seq = useRef(0);
  const { kbs, timelineKb: current, setTimelineKb } = useKb();
  const [moreOpen, setMoreOpen] = useState(false);
  const [tagQuery, setTagQuery] = useState("");
  // 多选(用于 AI 归并)与"可整理"提示
  const [selectMode, setSelectMode] = useState(false);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [hints, setHints] = useState<Record<string, number[]>>({});
  const { openMerge, openSuggest } = useOrganize();
  const toggle = useCallback((id: number) => setPicked((p) => { const n = new Set(p); if (n.has(id)) n.delete(id); else n.add(id); return n; }), []);
  const exitSelect = () => { setSelectMode(false); setPicked(new Set()); };

  useEffect(() => {
    const t = setTimeout(() => setDebounced(search), 250);
    return () => clearTimeout(t);
  }, [search]);

  const load = useCallback(async (append: boolean, before?: number) => {
    const my = ++seq.current;
    setLoading(true);
    try {
      const rows = await api.listItems({ kind, tag: tag ?? undefined, search: debounced || undefined, beforeId: before, limit: PAGE, kbId: current ?? undefined, view: "timeline" });
      if (my !== seq.current) return;
      setItems((prev) => (append ? [...prev, ...rows] : rows));
      setHasMore(rows.length === PAGE);
    } catch (e) {
      toast(`加载失败:${(e as Error).message}`, "err");
    } finally {
      if (my === seq.current) setLoading(false);
    }
  }, [kind, tag, debounced, current, toast]);

  useEffect(() => { void load(false); }, [load, dataVersion]);

  // 滚动到底部哨兵时自动加载下一页(游标分页,不用一次渲染全部)
  const sentinel = useRef<HTMLDivElement>(null);
  const loadMore = useRef<() => void>(() => undefined);
  loadMore.current = () => { if (hasMore && !loading) void load(true, items[items.length - 1]?.id); };
  useEffect(() => {
    const el = sentinel.current;
    if (!el) return;
    const io = new IntersectionObserver((es) => { if (es.some((e) => e.isIntersecting)) loadMore.current(); }, { rootMargin: "400px" });
    io.observe(el);
    return () => io.disconnect();
  }, [hasMore, items.length]);
  useEffect(() => { api.listTags().then(setTags).catch(() => undefined); }, [dataVersion]);

  // 已处理完的笔记:向后端询问是否有高度相似的碎片(只用已有向量,不调用模型)
  useEffect(() => {
    const ids = items.filter((i) => i.kind === "note" && i.status === "done").slice(0, 80).map((i) => i.id);
    if (ids.length === 0) { setHints({}); return; }
    let alive = true;
    api.organizeHints(ids).then((h) => { if (alive) setHints(h); }).catch(() => undefined);
    return () => { alive = false; };
  }, [items, dataVersion]);

  const groups = groupBy(items, (i) => fmtDayLabel(i.createdAt));
  const failed = status?.queue.failed ?? 0;
  const folded = foldTags(tags, tag, 7);

  const chip = (active: boolean) =>
    `rounded-full px-3 py-1 text-xs ${active ? "bg-[var(--accent)] text-[var(--on-accent)]" : "bg-[var(--bg-soft)] text-[var(--fg-dim)] hover:text-[var(--fg)]"}`;

  return (
    <div className="flex h-full flex-col">
      <div data-tauri-drag-region className="flex h-[52px] shrink-0 items-center gap-3 px-6">
        <h2 className="text-[15px] font-semibold" data-tauri-drag-region>记忆时间流</h2>
        <span className="text-xs text-[var(--fg-faint)]" data-tauri-drag-region>共 {status?.stats.notes ?? 0} 条记录 · {status?.stats.images ?? 0} 张图片</span>
        <div className="flex-1" data-tauri-drag-region />
        <button className="btn btn-secondary" title="找出最近相近的零散记录,让 AI 合并成主题笔记" onClick={openSuggest}><SparkleIcon size={13} />整理碎片</button>
        <button className={`btn ${selectMode ? "btn-primary" : "btn-secondary"}`} title="选择多条笔记后 AI 归并" onClick={() => (selectMode ? exitSelect() : setSelectMode(true))}>{selectMode ? "完成选择" : "选择"}</button>
        {failed > 0 && (
          <button className="rounded-lg bg-[var(--err)]/10 px-3 py-1 text-xs text-[var(--err)]" onClick={async () => { try { const n = await api.retryFailed(null); toast(`已重新排队 ${n} 项`, "ok"); bumpData(); void refreshStatus(); } catch (e) { toast((e as Error).message, "err"); } }}>
            重试 {failed} 项失败
          </button>
        )}
        <div className="flex w-56 items-center gap-2 rounded-lg bg-[var(--bg-soft)] px-2.5 py-1.5">
          <SearchIcon size={14} className="text-[var(--fg-faint)]" />
          <input className="w-full bg-transparent text-[13px] outline-none" placeholder="搜索记录" value={search} onChange={(e) => setSearch(e.target.value)} />
        </div>
      </div>

      <div className="flex items-center gap-2 px-6 pb-3">
        <button className={chip(kind === "note,image")} onClick={() => setKind("note,image")}>全部</button>
        <button className={chip(kind === "note")} onClick={() => setKind("note")}>文字</button>
        <button className={chip(kind === "image")} onClick={() => setKind("image")}>图片</button>
        {kbs.length > 1 && (
          <select
            aria-label="按知识库筛选"
            title="只看某个知识库的记录"
            className="select h-[26px] max-w-[150px] py-0 text-xs"
            value={current ?? ""}
            onChange={(e) => setTimelineKb(e.target.value === "" ? null : Number(e.target.value))}
          >
            <option value="">全部知识库</option>
            {kbs.map((k) => <option key={k.id} value={k.id}>{k.name}</option>)}
          </select>
        )}
        <span className="mx-1 h-4 w-px bg-[var(--line)]" />
        {tag && (
          <button className="flex items-center gap-1 rounded-full bg-[var(--accent)] px-3 py-1 text-xs text-[var(--on-accent)]" onClick={() => setTag(null)}>#{tag}<CloseIcon size={11} /></button>
        )}
        <div className="relative flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
          {folded.visible.map((t) => (
            <button key={t.tag} className="rounded-full border border-[var(--line)] px-2.5 py-1 text-xs text-[var(--fg-dim)] hover:border-[var(--accent)] hover:text-[var(--accent)]" onClick={() => setTag(t.tag)}>
              #{t.tag} <span className="text-[var(--fg-faint)]">{t.count}</span>
            </button>
          ))}
          {folded.rest.length > 0 && (
            <button className="rounded-full border border-dashed border-[var(--line)] px-2.5 py-1 text-xs text-[var(--fg-dim)] hover:border-[var(--accent)] hover:text-[var(--accent)]" onClick={() => setMoreOpen((v) => !v)}>
              更多标签 {folded.rest.length}
            </button>
          )}
          {moreOpen && (
            <>
              <div className="fixed inset-0 z-20" onClick={() => setMoreOpen(false)} />
              <div className="pop-in absolute left-0 top-full z-30 mt-2 w-[420px] max-w-full rounded-xl border border-[var(--line)] bg-[var(--bg-elev)] p-3 shadow-xl">
                <div className="mb-2 flex items-center gap-2 rounded-lg bg-[var(--bg-soft)] px-2.5 py-1.5">
                  <SearchIcon size={13} className="text-[var(--fg-faint)]" />
                  <input autoFocus className="w-full bg-transparent text-[13px] outline-none" placeholder="搜索标签" value={tagQuery} onChange={(e) => setTagQuery(e.target.value)} />
                </div>
                <div className="flex max-h-56 flex-wrap gap-1.5 overflow-y-auto">
                  {filterTags([...folded.visible, ...folded.rest], tagQuery).map((t) => (
                    <button key={t.tag} className="rounded-full border border-[var(--line)] px-2.5 py-1 text-xs text-[var(--fg-dim)] hover:border-[var(--accent)] hover:text-[var(--accent)]" onClick={() => { setTag(t.tag); setMoreOpen(false); setTagQuery(""); }}>
                      #{t.tag} <span className="text-[var(--fg-faint)]">{t.count}</span>
                    </button>
                  ))}
                  {filterTags([...folded.visible, ...folded.rest], tagQuery).length === 0 && <span className="text-xs text-[var(--fg-dim)]">没有匹配的标签</span>}
                </div>
              </div>
            </>
          )}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-6 pb-8">
        <div className="mx-auto max-w-[760px]">
          {items.length === 0 && !loading && (
            <div className="pt-24 text-center text-sm text-[var(--fg-dim)]">
              {debounced || tag ? "没有符合条件的记录" : <>还没有记录。按下全局快捷键 <kbd className="rounded border border-[var(--line)] px-1.5">{status?.settings.shortcut.replace("CommandOrControl", "⌘").replace(/\+/g, " ")}</kbd> 随手记一句话,或拖入图片。</>}
            </div>
          )}
          {groups.map((g) => (
            <section key={g.key} className="mb-6">
              <div className="sticky top-0 z-10 -mx-1 mb-2 bg-[var(--bg)]/85 px-1 py-1.5 text-xs font-medium text-[var(--fg-dim)] backdrop-blur">{g.key}</div>
              <div className="flex flex-col gap-3">
                {g.items.map((it) => (
                  <ItemCard
                    key={it.id}
                    item={it}
                    selected={drawer?.itemId === it.id}
                    onTag={setTag}
                    selectMode={selectMode}
                    checked={picked.has(it.id)}
                    onToggle={toggle}
                    peers={hints[String(it.id)]}
                    onMerge={openMerge}
                  />
                ))}
              </div>
            </section>
          ))}
          {hasMore && (
            <div ref={sentinel} className="flex justify-center py-4">
              <button className="rounded-lg bg-[var(--bg-soft)] px-4 py-2 text-xs hover:bg-[var(--line)] disabled:opacity-50" disabled={loading} onClick={() => void load(true, items[items.length - 1]?.id)}>
                {loading ? "加载中…" : "加载更多"}
              </button>
            </div>
          )}
        </div>
      </div>

      {selectMode && (
        <div className="flex shrink-0 items-center gap-3 border-t border-[var(--line)] bg-[var(--bg)] px-6 py-2.5 text-xs">
          <span className="text-[var(--fg-dim)]">已选 {picked.size} 条文字笔记{picked.size > 0 && !mergeCountOk(picked.size) ? "(归并需要 2–12 条)" : ""}</span>
          <div className="flex-1" />
          <button className="btn btn-ghost" onClick={exitSelect}>取消</button>
          <button
            className="btn btn-primary"
            disabled={!mergeCountOk(picked.size)}
            onClick={() => { const ids = [...picked]; exitSelect(); openMerge(ids); }}
          >
            <SparkleIcon size={13} />AI 归并
          </button>
        </div>
      )}
    </div>
  );
}
