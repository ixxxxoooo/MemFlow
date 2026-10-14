// @author ygw
// 记忆时间流:按天分组的瀑布流,支持搜索与标签过滤

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { fmtDayLabel, groupBy } from "../../lib/format";
import { useApp } from "../../lib/store";
import type { Item, TagCount } from "../../lib/types";
import { CloseIcon, SearchIcon } from "../components/icons";
import { ItemCard } from "../components/ItemCard";

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

  useEffect(() => {
    const t = setTimeout(() => setDebounced(search), 250);
    return () => clearTimeout(t);
  }, [search]);

  const load = useCallback(async (append: boolean, before?: number) => {
    const my = ++seq.current;
    setLoading(true);
    try {
      const rows = await api.listItems({ kind, tag: tag ?? undefined, search: debounced || undefined, beforeId: before, limit: PAGE });
      if (my !== seq.current) return;
      setItems((prev) => (append ? [...prev, ...rows] : rows));
      setHasMore(rows.length === PAGE);
    } catch (e) {
      toast(`加载失败:${(e as Error).message}`, "err");
    } finally {
      if (my === seq.current) setLoading(false);
    }
  }, [kind, tag, debounced, toast]);

  useEffect(() => { void load(false); }, [load, dataVersion]);
  useEffect(() => { api.listTags().then(setTags).catch(() => undefined); }, [dataVersion]);

  const groups = groupBy(items, (i) => fmtDayLabel(i.createdAt));
  const failed = status?.queue.failed ?? 0;

  const chip = (active: boolean) =>
    `rounded-full px-3 py-1 text-xs ${active ? "bg-[var(--accent)] text-white" : "bg-[var(--bg-soft)] text-[var(--fg-dim)] hover:text-[var(--fg)]"}`;

  return (
    <div className="flex h-full flex-col">
      <div data-tauri-drag-region className="flex h-[52px] shrink-0 items-center gap-3 px-6">
        <h2 className="text-[15px] font-semibold" data-tauri-drag-region>记忆时间流</h2>
        <span className="text-xs text-[var(--fg-faint)]" data-tauri-drag-region>共 {status?.stats.notes ?? 0} 条记录 · {status?.stats.images ?? 0} 张图片</span>
        <div className="flex-1" data-tauri-drag-region />
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
        <span className="mx-1 h-4 w-px bg-[var(--line)]" />
        {tag && (
          <button className="flex items-center gap-1 rounded-full bg-[var(--accent)] px-3 py-1 text-xs text-white" onClick={() => setTag(null)}>#{tag}<CloseIcon size={11} /></button>
        )}
        <div className="flex min-w-0 flex-1 gap-1.5 overflow-x-auto">
          {tags.filter((t) => t.tag !== tag).slice(0, 14).map((t) => (
            <button key={t.tag} className="shrink-0 rounded-full border border-[var(--line)] px-2.5 py-1 text-xs text-[var(--fg-dim)] hover:border-[var(--accent)] hover:text-[var(--accent)]" onClick={() => setTag(t.tag)}>
              #{t.tag} <span className="text-[var(--fg-faint)]">{t.count}</span>
            </button>
          ))}
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
                {g.items.map((it) => <ItemCard key={it.id} item={it} selected={drawer?.itemId === it.id} onTag={setTag} />)}
              </div>
            </section>
          ))}
          {hasMore && (
            <div className="flex justify-center py-4">
              <button className="rounded-lg bg-[var(--bg-soft)] px-4 py-2 text-xs hover:bg-[var(--line)] disabled:opacity-50" disabled={loading} onClick={() => void load(true, items[items.length - 1]?.id)}>
                {loading ? "加载中…" : "加载更多"}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
