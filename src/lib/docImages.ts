// @author ixxxxoooo
// 文档预览里的图片:把正文中 `![](相对路径)` 的写法对应到导入时关联的图片记录

import type { ImageRef, LinkedImage } from "./types";

/** 预览用的图片信息 */
export interface DocImage {
  /** 可直接显示的 URL */
  url: string;
  /** 图片记录 id */
  id: number;
  name: string | null;
}

/** 规范化引用写法:解码百分号、去掉 ?query / #fragment、去掉前导 ./ */
export function imageKey(src: string): string {
  let s = src.trim().split(/[?#]/)[0] ?? "";
  try {
    s = decodeURIComponent(s);
  } catch {
    // 非法编码:保持原样
  }
  return s.replace(/^\.\//, "");
}

/**
 * 建立「引用写法 → 图片」映射
 *
 * 参数: images 文档关联的图片;urlOf 把附件相对路径转成可显示 URL
 */
export function buildImageMap(images: LinkedImage[], urlOf: (rel: string) => string): Map<string, DocImage> {
  const map = new Map<string, DocImage>();
  for (const im of images) {
    if (!im.attachmentPath) continue;
    map.set(imageKey(im.anchor), { url: urlOf(im.attachmentPath), id: im.childId, name: im.originalName });
  }
  return map;
}

/** 按正文里的写法查找图片(渲染器拿到的地址已被 Markdown 解析器做过百分号编码) */
export function lookupImage(map: Map<string, DocImage> | undefined, src: string | undefined): DocImage | undefined {
  if (!map || !src) return undefined;
  return map.get(imageKey(src));
}

/** 汇总多条引用来源里的插图(按图片 id 去重,保持出现顺序);旧消息没有 images 字段 */
export function uniqueImages(sources: { images?: ImageRef[] }[]): ImageRef[] {
  const seen = new Set<number>();
  const out: ImageRef[] = [];
  for (const s of sources) {
    for (const im of s.images ?? []) {
      if (!seen.has(im.id)) {
        seen.add(im.id);
        out.push(im);
      }
    }
  }
  return out;
}
