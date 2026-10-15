// @author ygw
// 关联图片的缩略图条:切片/引用来源里出现的插图,点击在右栏打开图片详情

import { useApp } from "../../lib/store";
import type { ImageRef } from "../../lib/types";
import { useAttachmentUrl } from "./ItemDrawer";

function Thumb({ image, size, onOpen }: { image: ImageRef; size: number; onOpen: (id: number) => void }) {
  const url = useAttachmentUrl(image.attachmentPath);
  return (
    <button
      type="button"
      title={image.name ?? "图片"}
      onClick={() => onOpen(image.id)}
      style={{ width: size, height: size }}
      className="shrink-0 overflow-hidden rounded-md border border-[var(--line)] bg-[var(--bg-soft)] hover:border-[var(--accent)]"
    >
      {url && <img src={url} alt={image.name ?? ""} loading="lazy" className="h-full w-full object-cover" />}
    </button>
  );
}

/** 缩略图条;超过 max 张时折叠显示剩余数量 */
export function ImageThumbs({ images, size = 56, max = 8 }: { images: ImageRef[]; size?: number; max?: number }) {
  const openDrawer = useApp((s) => s.openDrawer);
  if (images.length === 0) return null;
  const shown = images.slice(0, max);
  return (
    <div className="mt-2 flex flex-wrap items-center gap-1.5">
      {shown.map((im) => <Thumb key={im.id} image={im} size={size} onOpen={(id) => openDrawer(id)} />)}
      {images.length > max && <span className="text-[11px] text-[var(--fg-faint)]">+{images.length - max}</span>}
    </div>
  );
}
