// @author ixxxxoooo
// 导入支持的格式提示:一行摘要 + 点开查看每种格式的解析说明

import { useRef, useState } from "react";
import { DOC_FORMATS, FOLDER_NOTE, FORMATS_SUMMARY, IMAGE_FORMAT, UNSUPPORTED_NOTE } from "../../lib/formats";
import { Popover } from "./Popover";
import { InfoIcon } from "./icons";

/** 格式提示;compact 时只显示摘要文字(用于空状态) */
export function FormatsHint({ className = "" }: { className?: string }) {
  const btn = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  return (
    <>
      <button
        ref={btn}
        type="button"
        data-testid="formats-hint"
        title="查看支持导入的文件格式"
        className={`flex items-center gap-1.5 text-xs text-[var(--fg-dim)] hover:text-[var(--accent)] ${className}`}
        onClick={() => setAnchor(btn.current?.getBoundingClientRect() ?? null)}
      >
        <InfoIcon size={13} className="shrink-0" />
        <span className="truncate">支持 {FORMATS_SUMMARY},可直接导入整个文件夹</span>
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} width={400}>
          <div className="px-2.5 pb-1 pt-1 text-[10.5px] font-semibold tracking-wide text-[var(--fg-faint)]">支持导入的格式</div>
          <table className="w-full text-[12.5px]">
            <tbody>
              {[...DOC_FORMATS, IMAGE_FORMAT].map((f) => (
                <tr key={f.label} className="align-top">
                  <td className="whitespace-nowrap px-2.5 py-1 font-medium">{f.label}</td>
                  <td className="py-1 pr-1 text-[11px] text-[var(--fg-faint)]">.{f.exts.join(" .")}</td>
                  <td className="py-1 pr-2.5 text-[11.5px] text-[var(--fg-dim)]">{f.note}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="mt-1 space-y-1 border-t border-[var(--line)] px-2.5 pt-2 text-[11.5px] leading-snug text-[var(--fg-dim)]">
            <div>{FOLDER_NOTE}</div>
            <div>{UNSUPPORTED_NOTE}</div>
          </div>
        </Popover>
      )}
    </>
  );
}
