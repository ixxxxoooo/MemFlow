// @author ixxxxoooo
// 锚定在触发元素旁的浮层(挂到 body,不会被表格/滚动容器裁切),点击外部或按 Esc 关闭

import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";

interface Props {
  /** 触发元素的位置 */
  anchor: DOMRect;
  onClose: () => void;
  children: ReactNode;
  width?: number;
  /** 优先展开方向;空间不足时自动翻转 */
  placement?: "below" | "above";
  /** 水平对齐触发元素的左侧或右侧 */
  align?: "left" | "right";
}

const MARGIN = 8;

export function Popover({ anchor, onClose, children, width = 220, placement = "below", align = "left" }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const [style, setStyle] = useState<CSSProperties>({ visibility: "hidden", width });

  // 量出内容高度后再决定向上还是向下展开,并限制在窗口内
  useLayoutEffect(() => {
    const h = box.current?.offsetHeight ?? 0;
    const below = window.innerHeight - anchor.bottom - MARGIN;
    const above = anchor.top - MARGIN;
    const openBelow = placement === "below" ? below >= h || below >= above : !(above >= h || above >= below);
    const left = align === "left" ? anchor.left : anchor.right - width;
    setStyle({
      width,
      left: Math.max(MARGIN, Math.min(left, window.innerWidth - width - MARGIN)),
      ...(openBelow ? { top: anchor.bottom + 4, maxHeight: Math.max(120, below) } : { bottom: window.innerHeight - anchor.top + 4, maxHeight: Math.max(120, above) }),
    });
  }, [anchor, width, placement, align]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return createPortal(
    <>
      <div className="fixed inset-0 z-[60]" onMouseDown={onClose} />
      <div
        ref={box}
        className="pop-in fixed z-[61] overflow-y-auto rounded-xl border border-[var(--line)] bg-[var(--bg-elev)] p-1.5 shadow-xl"
        style={style}
      >
        {children}
      </div>
    </>,
    document.body,
  );
}
