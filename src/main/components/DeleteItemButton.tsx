// @author ixxxxoooo
// 列表项上的悬停删除按钮:二次确认 → 删除 → 乐观关闭相关抽屉并刷新

import { api } from "../../lib/api";
import { confirmDialog } from "../../lib/confirm";
import { useApp } from "../../lib/store";
import { TrashIcon } from "./icons";

/**
 * 悬停显示的删除按钮。
 * 父元素需带 `group`(或具名分组并通过 revealClass 传入对应的 group-hover 类)才会在悬停时显现;
 * 键盘聚焦时同样可见,保证可访问性。
 */
export function DeleteItemButton({
  id,
  confirmText = "确定删除这条记录吗?此操作不可恢复。",
  className = "",
  revealClass = "opacity-0 group-hover:opacity-100",
}: {
  id: number;
  confirmText?: string;
  className?: string;
  revealClass?: string;
}) {
  const { toast, itemsDeleted, refreshStatus } = useApp();
  return (
    <button
      type="button"
      aria-label="删除"
      title="删除"
      className={`flex h-7 w-7 items-center justify-center rounded-full text-[var(--fg-dim)] transition-opacity hover:bg-[var(--err)]/12 hover:text-[var(--err)] focus-visible:opacity-100 focus-visible:outline-2 focus-visible:outline-[var(--accent)] ${revealClass} ${className}`}
      onClick={async (e) => {
        // 卡片/行本身可点击打开详情,这里不能冒泡
        e.stopPropagation();
        if (!(await confirmDialog(confirmText))) return;
        try {
          await api.deleteItems([id]);
          itemsDeleted([id]);
          toast("已删除", "ok");
          void refreshStatus();
        } catch (err) {
          toast((err as Error).message, "err");
        }
      }}
    >
      <TrashIcon size={14} />
    </button>
  );
}
