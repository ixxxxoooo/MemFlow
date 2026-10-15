// @author ixxxxoooo
// 设置页通用的一行:左侧名称与说明,右侧控件

import type { ReactNode } from "react";

export function Row({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-6 border-b border-[var(--line)] py-3.5 last:border-0">
      <div className="max-w-[300px]">
        <div className="text-[13.5px] font-medium">{label}</div>
        {hint && <div className="mt-0.5 text-xs leading-snug text-[var(--fg-dim)]">{hint}</div>}
      </div>
      <div className="flex shrink-0 items-center">{children}</div>
    </div>
  );
}

export const inputCls = "w-60 rounded-lg border border-[var(--line)] bg-[var(--bg-soft)] px-2.5 py-1.5 text-[13px] outline-none focus:border-[var(--accent)]";
