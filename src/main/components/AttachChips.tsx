// @author ixxxxoooo
// 待保存的附件小卡片(图片缩略图 / 文件图标),悬浮时显示移除按钮

import type { Attach } from "../../lib/useCapture";
import { CloseIcon, FileIcon, ImageIcon } from "./icons";

export function AttachChips({ files, onRemove, className = "" }: { files: Attach[]; onRemove: (id: number) => void; className?: string }) {
  if (files.length === 0) return null;
  return (
    <div className={`flex flex-wrap gap-2 ${className}`}>
      {files.map((f) => (
        <div key={f.id} className="group relative flex items-center gap-2 overflow-hidden rounded-xl border border-[var(--line)] bg-[var(--bg-soft)]/70 p-1.5 pr-3 text-xs">
          {f.preview ? (
            <img src={f.preview} alt="" className="h-12 w-12 rounded-lg object-cover" />
          ) : /\.(png|jpe?g|webp|gif|bmp)$/i.test(f.name) ? (
            <ImageIcon size={18} className="m-3 text-[var(--accent)]" />
          ) : (
            <FileIcon size={18} className="m-3 text-[var(--accent)]" />
          )}
          <span className="max-w-[170px] truncate">{f.name}</span>
          <button aria-label={`移除 ${f.name}`} className="absolute right-1 top-1 hidden rounded-full bg-black/55 p-0.5 text-white group-hover:block" onClick={() => onRemove(f.id)}>
            <CloseIcon size={10} />
          </button>
        </div>
      ))}
    </div>
  );
}
