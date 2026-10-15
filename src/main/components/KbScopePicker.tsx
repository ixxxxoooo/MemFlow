// @author ygw
// 问答范围选择:输入框旁的「@」按钮(多选)与键入 @ 时弹出的候选列表

import { useRef, useState } from "react";
import { normalizeScope, toggleScope } from "../../lib/kbScope";
import { useChat } from "../../lib/chatStore";
import { useKb } from "../../lib/kbStore";
import type { Kb } from "../../lib/types";
import { Popover } from "./Popover";
import { AtIcon, CheckIcon, FolderIcon } from "./icons";

/** 输入框旁的范围按钮:点开后多选知识库;一个都不选 = 搜索全部 */
export function KbScopeButton() {
  const { kbs } = useKb();
  const { kbIds, setKbIds, busy } = useChat();
  const btn = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  const scope = normalizeScope(kbIds, kbs);

  // 只有一个知识库时没有选择的必要
  if (kbs.length < 2) return null;

  return (
    <>
      <button
        ref={btn}
        type="button"
        disabled={busy}
        title="限定提问的知识库范围(也可在输入框里键入 @)"
        className={`flex h-9 items-center gap-1 rounded-xl px-2.5 text-[12.5px] hover:bg-[var(--bg-soft)] disabled:opacity-50 ${scope.length ? "text-[var(--accent)]" : "text-[var(--fg-dim)]"}`}
        onClick={() => setAnchor(btn.current?.getBoundingClientRect() ?? null)}
      >
        <AtIcon size={15} />
        <span>{scope.length ? `${scope.length} 个库` : "全部库"}</span>
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} width={230} placement="above">
          <div className="px-2.5 pb-1 pt-1 text-[10.5px] font-semibold tracking-wide text-[var(--fg-faint)]">只在这些知识库里找</div>
          <button
            className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-[var(--bg-soft)]"
            onClick={() => setKbIds([])}
          >
            <span className="flex h-4 w-4 items-center justify-center">{scope.length === 0 && <CheckIcon size={13} className="text-[var(--accent)]" />}</span>
            <span className="flex-1">全部知识库(不限定)</span>
          </button>
          {kbs.map((k) => (
            <button
              key={k.id}
              className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-[var(--bg-soft)]"
              onClick={() => setKbIds(toggleScope(scope, k.id))}
            >
              <span className="flex h-4 w-4 items-center justify-center">{scope.includes(k.id) && <CheckIcon size={13} className="text-[var(--accent)]" />}</span>
              <span className="min-w-0 flex-1 truncate">{k.name}</span>
              <span className="text-[11px] text-[var(--fg-faint)]">{k.itemCount}</span>
            </button>
          ))}
        </Popover>
      )}
    </>
  );
}

/** 键入 @ 时在输入框上方弹出的候选;↑↓ 选择、Enter/Tab 确认由父组件的键盘处理驱动 */
export function KbMentionPopup({
  candidates,
  selected,
  highlight,
  onPick,
}: {
  candidates: Kb[];
  selected: number[];
  highlight: number;
  onPick: (kb: Kb) => void;
}) {
  return (
    <div className="pop-in absolute bottom-full left-2 z-30 mb-2 max-h-64 w-64 overflow-y-auto rounded-xl border border-[var(--line)] bg-[var(--bg-elev)] p-1.5 shadow-xl">
      <div className="px-2.5 pb-1 pt-1 text-[10.5px] font-semibold tracking-wide text-[var(--fg-faint)]">选择知识库 · ↑↓ 切换 · Enter 确认</div>
      {candidates.length === 0 && <div className="px-2.5 py-2 text-xs text-[var(--fg-dim)]">没有匹配的知识库</div>}
      {candidates.map((k, i) => (
        <button
          key={k.id}
          // 用 mousedown 而不是 click,避免输入框先失焦导致候选被关闭
          onMouseDown={(e) => { e.preventDefault(); onPick(k); }}
          className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] ${i === highlight ? "bg-[var(--accent-soft)]" : "hover:bg-[var(--bg-soft)]"}`}
        >
          <FolderIcon size={13} className="shrink-0 text-[var(--accent)]" />
          <span className="min-w-0 flex-1 truncate">{k.name}</span>
          {selected.includes(k.id) && <CheckIcon size={13} className="text-[var(--accent)]" />}
        </button>
      ))}
    </div>
  );
}
