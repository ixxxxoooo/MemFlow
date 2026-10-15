// @author ygw
// 侧栏「当前知识库」切换器:切换范围、新建、重命名、删除(可选转移或级联)

import { useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import type { Kb } from "../../lib/types";
import { Popover } from "./Popover";
import { CheckIcon, ChevronDownIcon, EditIcon, FolderIcon, PlusIcon, TrashIcon } from "./icons";

/** 删除知识库对话框:选择库内记录的去向 */
function DeleteKbDialog({ kb, onClose }: { kb: Kb; onClose: () => void }) {
  const remove = useKb((s) => s.remove);
  const { toast, bumpData, refreshStatus } = useApp();
  const [mode, setMode] = useState<"moveToDefault" | "cascade">("moveToDefault");
  const [busy, setBusy] = useState(false);

  const confirm = async () => {
    setBusy(true);
    try {
      const n = await remove(kb.id, mode);
      toast(mode === "cascade" ? `已删除知识库「${kb.name}」及其 ${n} 条记录` : `已删除知识库「${kb.name}」,${n} 条记录已转入默认知识库`, "ok");
      bumpData();
      void refreshStatus();
      onClose();
    } catch (e) {
      toast((e as Error).message, "err");
      setBusy(false);
    }
  };

  const opt = (value: typeof mode, title: string, desc: string) => (
    <label className={`flex cursor-pointer items-start gap-2.5 rounded-xl border p-3 ${mode === value ? "border-[var(--accent)] bg-[var(--accent-soft)]" : "border-[var(--line)]"}`}>
      <input type="radio" className="mt-1" checked={mode === value} onChange={() => setMode(value)} />
      <span>
        <span className="block text-[13px] font-medium">{title}</span>
        <span className="block text-xs text-[var(--fg-dim)]">{desc}</span>
      </span>
    </label>
  );

  return createPortal(
    <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/45 p-6" onMouseDown={onClose}>
      <div className="pop-in w-[400px] rounded-2xl border border-[var(--line)] bg-[var(--bg)] p-5 shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <div className="text-[15px] font-semibold">删除知识库「{kb.name}」</div>
        <div className="mt-1 text-xs text-[var(--fg-dim)]">库内共有 {kb.itemCount} 条记录,请选择它们的去向:</div>
        <div className="mt-3 space-y-2">
          {opt("moveToDefault", "转移到默认知识库(推荐)", "记录保留,只是换了归属;默认库已有相同文件的会被去重")}
          {opt("cascade", "连同记录一起删除", "库内所有记录、向量与无人引用的附件都会被删除,不可恢复")}
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <button className="btn btn-secondary" onClick={onClose} disabled={busy}>取消</button>
          <button className={`btn ${mode === "cascade" ? "bg-[var(--err)] text-white" : "btn-primary"}`} onClick={() => void confirm()} disabled={busy}>
            {busy ? "处理中…" : mode === "cascade" ? "删除库和记录" : "删除知识库"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

export function KbSwitcher() {
  const { kbs, current, setCurrent, create, rename } = useKb();
  const toast = useApp((s) => s.toast);
  const btn = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  /** 正在编辑名称的知识库;"new" 表示新建 */
  const [editing, setEditing] = useState<number | "new" | null>(null);
  const [name, setName] = useState("");
  const [deleting, setDeleting] = useState<Kb | null>(null);

  const total = kbs.reduce((n, k) => n + k.itemCount, 0);
  const label = current == null ? "全部知识库" : kbs.find((k) => k.id === current)?.name ?? "全部知识库";

  const close = () => {
    setAnchor(null);
    setEditing(null);
  };

  const submitName = async () => {
    const n = name.trim();
    if (!n) return;
    try {
      if (editing === "new") {
        const kb = await create(n);
        setCurrent(kb.id);
        toast(`已创建知识库「${kb.name}」`, "ok");
      } else if (editing != null) {
        await rename(editing, n);
      }
      setEditing(null);
      setName("");
    } catch (e) {
      toast((e as Error).message, "err");
    }
  };

  const nameInput = (
    <input
      autoFocus
      className="w-full rounded-md border border-[var(--accent)] bg-[var(--bg)] px-2 py-1 text-[13px] outline-none"
      value={name}
      maxLength={40}
      placeholder="知识库名称"
      onChange={(e) => setName(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !e.nativeEvent.isComposing) void submitName();
        if (e.key === "Escape") { e.stopPropagation(); setEditing(null); }
      }}
    />
  );

  const row = (id: number | null, text: string, count: number, kb?: Kb) => (
    <div key={id ?? "all"} className={`group/kb flex items-center gap-1 rounded-lg px-1.5 ${current === id ? "bg-[var(--accent-soft)]" : "hover:bg-[var(--bg-soft)]"}`}>
      {kb && editing === kb.id ? (
        <div className="flex-1 py-1">{nameInput}</div>
      ) : (
        <button className="flex min-w-0 flex-1 items-center gap-2 px-1 py-1.5 text-left text-[13px]" onClick={() => { setCurrent(id); close(); }}>
          <span className="flex h-4 w-4 shrink-0 items-center justify-center">{current === id && <CheckIcon size={13} className="text-[var(--accent)]" />}</span>
          <span className="min-w-0 flex-1 truncate">{text}</span>
          <span className="text-[11px] text-[var(--fg-faint)]">{count}</span>
        </button>
      )}
      {kb && editing !== kb.id && (
        <span className="hidden items-center group-hover/kb:flex">
          <button title="重命名" className="rounded p-1 text-[var(--fg-dim)] hover:text-[var(--fg)]" onClick={() => { setName(kb.name); setEditing(kb.id); }}><EditIcon size={12} /></button>
          {kb.id !== 1 && (
            <button title="删除" className="rounded p-1 text-[var(--fg-dim)] hover:text-[var(--err)]" onClick={() => { close(); setDeleting(kb); }}><TrashIcon size={12} /></button>
          )}
        </span>
      )}
    </div>
  );

  return (
    <div className="px-2.5 pb-2">
      <div className="px-1 pb-1 text-[10.5px] font-semibold tracking-wide text-[var(--fg-faint)]">当前知识库</div>
      <button
        ref={btn}
        className="flex w-full items-center gap-2 rounded-lg border border-[var(--line)] bg-[var(--bg-elev)]/60 px-2.5 py-1.5 text-left text-[13px] hover:border-[var(--accent)]"
        title="切换知识库:时间流与档案库只显示当前库,导入内容归入当前库"
        onClick={() => setAnchor(btn.current?.getBoundingClientRect() ?? null)}
      >
        <FolderIcon size={14} className="shrink-0 text-[var(--accent)]" />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        <ChevronDownIcon size={13} className="shrink-0 text-[var(--fg-faint)]" />
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={close} width={Math.max(anchor.width, 220)}>
          {row(null, "全部知识库", total)}
          {kbs.map((k) => row(k.id, k.name, k.itemCount, k))}
          <div className="mt-1 border-t border-[var(--line)] pt-1">
            {editing === "new" ? (
              <div className="px-1.5 py-1">{nameInput}</div>
            ) : (
              <button className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] text-[var(--accent)] hover:bg-[var(--bg-soft)]" onClick={() => { setName(""); setEditing("new"); }}>
                <PlusIcon size={13} />新建知识库
              </button>
            )}
          </div>
        </Popover>
      )}
      {deleting && <DeleteKbDialog kb={deleting} onClose={() => setDeleting(null)} />}
    </div>
  );
}
