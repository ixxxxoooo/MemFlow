// @author ixxxxoooo
// 主窗口轻提示

import { useApp } from "../../lib/store";

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  return (
    <div className="pointer-events-none fixed bottom-5 left-1/2 z-[60] flex -translate-x-1/2 flex-col items-center gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`pop-in max-w-[520px] rounded-xl border px-4 py-2 text-[13px] shadow-lg backdrop-blur ${
            t.kind === "err"
              ? "border-[var(--err)]/40 bg-[color-mix(in_srgb,var(--bg-elev)_88%,var(--err))]"
              : t.kind === "ok"
                ? "border-[var(--ok)]/40 bg-[color-mix(in_srgb,var(--bg-elev)_90%,var(--ok))]"
                : "border-[var(--line)] bg-[var(--bg-elev)]"
          }`}
        >
          {t.text}
        </div>
      ))}
    </div>
  );
}
