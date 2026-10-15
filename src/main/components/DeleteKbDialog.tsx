// @author ygw
// 删除知识库对话框:选择库内记录的去向(转移到默认库或连同记录删除)

import { useState } from "react";
import { createPortal } from "react-dom";
import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import type { Kb } from "../../lib/types";

/** 删除知识库对话框:选择库内记录的去向 */
export function DeleteKbDialog({ kb, onClose }: { kb: Kb; onClose: () => void }) {
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
