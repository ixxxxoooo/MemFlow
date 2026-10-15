// @author ixxxxoooo
// 可缩放、可拖拽的容器(用于放大查看流程图)

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { wheelIntent } from "../../lib/panzoom";

interface Props {
  children: ReactNode;
  className?: string;
}

const MIN_K = 0.2;
const MAX_K = 8;

/** 普通滚轮平移 + ⌘/Ctrl 滚轮(触控板捏合)缩放 + 按住拖拽 + 双击复位 */
export function PanZoom({ children, className = "" }: Props) {
  const [t, setT] = useState({ x: 0, y: 0, k: 1 });
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);
  const box = useRef<HTMLDivElement>(null);

  const zoomBy = useCallback((factor: number, cx?: number, cy?: number) => {
    setT((p) => {
      const k = Math.min(MAX_K, Math.max(MIN_K, p.k * factor));
      const r = box.current?.getBoundingClientRect();
      const px = (cx ?? (r ? r.left + r.width / 2 : 0)) - (r?.left ?? 0);
      const py = (cy ?? (r ? r.top + r.height / 2 : 0)) - (r?.top ?? 0);
      const s = k / p.k;
      return { k, x: px - (px - p.x) * s, y: py - (py - p.y) * s };
    });
  }, []);

  // 滚轮必须以 passive:false 注册,preventDefault 才能阻止背后页面一起滚动
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const intent = wheelIntent(e);
      if (intent.type === "zoom") zoomBy(intent.factor, e.clientX, e.clientY);
      else setT((p) => ({ ...p, x: p.x + intent.dx, y: p.y + intent.dy }));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoomBy]);

  return (
    <div className={`relative overflow-hidden ${className}`} style={{ overscrollBehavior: "contain" }}>
      <div
        ref={box}
        className="h-full w-full cursor-grab touch-none active:cursor-grabbing"
        onPointerDown={(e) => {
          (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
          drag.current = { x: e.clientX, y: e.clientY, ox: t.x, oy: t.y };
        }}
        onPointerMove={(e) => {
          const d = drag.current;
          if (d) setT((p) => ({ ...p, x: d.ox + e.clientX - d.x, y: d.oy + e.clientY - d.y }));
        }}
        onPointerUp={() => (drag.current = null)}
        onDoubleClick={() => setT({ x: 0, y: 0, k: 1 })}
      >
        <div
          className="flex h-full w-full items-center justify-center"
          style={{ transform: `translate(${t.x}px, ${t.y}px) scale(${t.k})`, transformOrigin: "0 0" }}
        >
          {children}
        </div>
      </div>
      <div className="absolute bottom-3 right-3 flex items-center gap-1 rounded-lg border border-[var(--line)] bg-[var(--bg-elev)] p-1 text-sm shadow">
        <span className="px-1.5 text-[10px] text-[var(--fg-dim)]">滚轮平移 · ⌘/Ctrl+滚轮缩放</span>
        <button className="h-7 w-7 rounded hover:bg-[var(--bg-soft)]" onClick={() => zoomBy(1 / 1.25)}>−</button>
        <span className="w-11 text-center text-xs text-[var(--fg-dim)]">{Math.round(t.k * 100)}%</span>
        <button className="h-7 w-7 rounded hover:bg-[var(--bg-soft)]" onClick={() => zoomBy(1.25)}>+</button>
        <button className="h-7 rounded px-2 text-xs hover:bg-[var(--bg-soft)]" onClick={() => setT({ x: 0, y: 0, k: 1 })}>复位</button>
      </div>
    </div>
  );
}
