// @author ixxxxoooo
// 问答范围选择器:放在输入框左上角上方的小胶囊,点开多选知识库;一个都不选 = 搜索全部

import { useRef, useState } from "react";
import { normalizeScope, scopeLabel, toggleScope } from "../../lib/kbScope";
import { useChat } from "../../lib/chatStore";
import { useKb } from "../../lib/kbStore";
import { Popover } from "./Popover";
import { CheckIcon, ChevronDownIcon, FolderIcon } from "./icons";

export function ChatKbPicker() {
  const kbs = useKb((s) => s.kbs);
  const { kbIds, setKbIds, busy } = useChat();
  const btn = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  const scope = normalizeScope(kbIds, kbs);

  // 只有一个知识库时没有选择的必要
  if (kbs.length < 2) return null;

  const rowCls = "flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-[var(--bg-soft)]";

  return (
    <div className="mx-auto mb-1.5 flex max-w-[760px] items-center px-1">
      <button
        ref={btn}
        type="button"
        data-testid="chat-kb-picker"
        disabled={busy}
        title="选择提问的知识库范围(可多选,不选 = 搜索全部知识库)"
        className={`flex max-w-[320px] items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs disabled:opacity-50 ${
          scope.length ? "border-[var(--accent)] bg-[var(--accent-soft)] text-[var(--accent)]" : "border-[var(--line)] bg-[var(--bg-elev)]/70 text-[var(--fg-dim)] hover:border-[var(--accent)]"
        }`}
        onClick={() => setAnchor(btn.current?.getBoundingClientRect() ?? null)}
      >
        <FolderIcon size={13} className="shrink-0" />
        <span className="shrink-0 text-[var(--fg-faint)]">知识库</span>
        <span className="min-w-0 truncate font-medium">{scopeLabel(scope, kbs)}</span>
        <ChevronDownIcon size={12} className="shrink-0" />
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} width={250} placement="above">
          <div className="px-2.5 pb-1 pt-1 text-[10.5px] font-semibold tracking-wide text-[var(--fg-faint)]">只在这些知识库里找</div>
          <button className={rowCls} onClick={() => setKbIds([])}>
            <span className="flex h-4 w-4 items-center justify-center">{scope.length === 0 && <CheckIcon size={13} className="text-[var(--accent)]" />}</span>
            <span className="flex-1">全部知识库</span>
          </button>
          {kbs.map((k) => (
            <button key={k.id} className={rowCls} onClick={() => setKbIds(toggleScope(scope, k.id))}>
              <span className="flex h-4 w-4 items-center justify-center">{scope.includes(k.id) && <CheckIcon size={13} className="text-[var(--accent)]" />}</span>
              <span className="min-w-0 flex-1 truncate">{k.name}</span>
              <span className="text-[11px] text-[var(--fg-faint)]">{k.documentCount} 文档</span>
            </button>
          ))}
        </Popover>
      )}
    </div>
  );
}
