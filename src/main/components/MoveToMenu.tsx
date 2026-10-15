// @author ixxxxoooo
// 「转移到…」菜单:选择目标知识库后回调;当前所在库标灰不可选

import { useRef, useState, type ReactNode } from "react";
import { useKb } from "../../lib/kbStore";
import { Popover } from "./Popover";
import { CheckIcon } from "./icons";

interface Props {
  /** 选定目标知识库 */
  onPick: (kbId: number) => void;
  /** 触发按钮内容 */
  children: ReactNode;
  className?: string;
  title?: string;
  /** 这些知识库不可选(如记录当前所在库) */
  disabledIds?: number[];
  align?: "left" | "right";
}

export function MoveToMenu({ onPick, children, className = "", title = "转移到其他知识库", disabledIds = [], align = "left" }: Props) {
  const kbs = useKb((s) => s.kbs);
  const btn = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<DOMRect | null>(null);

  return (
    <>
      <button
        ref={btn}
        type="button"
        title={title}
        className={className}
        onClick={(e) => {
          e.stopPropagation();
          setAnchor(btn.current?.getBoundingClientRect() ?? null);
        }}
      >
        {children}
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} width={210} align={align}>
          <div className="px-2.5 pb-1 pt-1 text-[10.5px] font-semibold tracking-wide text-[var(--fg-faint)]">转移到</div>
          {kbs.map((k) => {
            const disabled = disabledIds.includes(k.id);
            return (
              <button
                key={k.id}
                disabled={disabled}
                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-[var(--bg-soft)] disabled:opacity-45 disabled:hover:bg-transparent"
                onClick={(e) => {
                  e.stopPropagation();
                  setAnchor(null);
                  onPick(k.id);
                }}
              >
                <span className="min-w-0 flex-1 truncate">{k.name}</span>
                {disabled ? <CheckIcon size={13} className="text-[var(--fg-faint)]" /> : <span className="text-[11px] text-[var(--fg-faint)]">{k.itemCount}</span>}
              </button>
            );
          })}
        </Popover>
      )}
    </>
  );
}
