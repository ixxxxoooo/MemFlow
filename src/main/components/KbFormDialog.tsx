// @author ygw
// 新建 / 编辑知识库对话框:名称(必填)与描述(可选)

import { useState } from "react";
import { createPortal } from "react-dom";
import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import type { Kb } from "../../lib/types";

/** kb 为空表示新建;否则编辑该库的名称与描述 */
export function KbFormDialog({ kb, onClose, onSaved }: { kb?: Kb; onClose: () => void; onSaved?: (kb: Kb) => void }) {
  const { create, update } = useKb();
  const toast = useApp((s) => s.toast);
  const [name, setName] = useState(kb?.name ?? "");
  const [desc, setDesc] = useState(kb?.description ?? "");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const n = name.trim();
    if (!n || busy) return;
    setBusy(true);
    try {
      if (kb) {
        await update(kb.id, n, desc.trim());
        toast(`已保存「${n}」`, "ok");
        onSaved?.({ ...kb, name: n, description: desc.trim() });
      } else {
        const created = await create(n, desc.trim());
        toast(`已创建知识库「${created.name}」`, "ok");
        onSaved?.(created);
      }
      onClose();
    } catch (e) {
      toast((e as Error).message, "err");
      setBusy(false);
    }
  };

  const field = "w-full rounded-lg border border-[var(--line)] bg-[var(--bg-soft)] px-2.5 py-1.5 text-[13px] outline-none focus:border-[var(--accent)]";
  return createPortal(
    <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/45 p-6" onMouseDown={onClose}>
      <div role="dialog" aria-label={kb ? "编辑知识库" : "新建知识库"} className="pop-in w-[400px] rounded-2xl border border-[var(--line)] bg-[var(--bg)] p-5 shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <div className="text-[15px] font-semibold">{kb ? "编辑知识库" : "新建知识库"}</div>
        <label className="mt-3 block text-xs text-[var(--fg-dim)]">
          名称
          <input
            autoFocus
            className={`${field} mt-1`}
            value={name}
            maxLength={40}
            placeholder="例如:工作、读书笔记"
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) void submit(); if (e.key === "Escape") onClose(); }}
          />
        </label>
        <label className="mt-3 block text-xs text-[var(--fg-dim)]">
          描述(可选)
          <textarea className={`${field} mt-1 h-20 resize-none`} value={desc} maxLength={200} placeholder="这个知识库放什么内容" onChange={(e) => setDesc(e.target.value)} />
        </label>
        <div className="mt-4 flex justify-end gap-2">
          <button className="btn btn-secondary" onClick={onClose} disabled={busy}>取消</button>
          <button className="btn btn-primary" onClick={() => void submit()} disabled={busy || !name.trim()}>{busy ? "保存中…" : "保存"}</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
