// @author ygw
import { describe, expect, it } from "vitest";
import { buildImageMap, imageKey, lookupImage, uniqueImages } from "./docImages";
import type { LinkedImage } from "./types";

const im = (anchor: string, id: number, path: string | null = "ab/x.png"): LinkedImage => ({
  childId: id,
  anchor,
  seq: 0,
  originalName: "x.png",
  attachmentPath: path,
  ocrText: null,
  visionDesc: null,
});

describe("docImages", () => {
  it("规范化引用写法", () => {
    expect(imageKey("./img/a.png?raw=1#top")).toBe("img/a.png");
    expect(imageKey("img/my%20pic.png")).toBe("img/my pic.png");
    expect(imageKey("100%.png")).toBe("100%.png");
  });

  it("百分号编码与原始写法能互相对应", () => {
    const map = buildImageMap([im("../img/预算 图.png", 3), im("media/image1.png", 4), im("无附件.png", 5, null)], (rel) => `asset://${rel}`);
    // Markdown 解析器会把中文与空格编码后再交给渲染器
    expect(lookupImage(map, "../img/%E9%A2%84%E7%AE%97%20%E5%9B%BE.png")?.id).toBe(3);
    expect(lookupImage(map, "media/image1.png")?.url).toBe("asset://ab/x.png");
    expect(lookupImage(map, "无附件.png")).toBeUndefined();
    expect(lookupImage(undefined, "a.png")).toBeUndefined();
    expect(lookupImage(map, undefined)).toBeUndefined();
  });

  it("汇总来源插图并去重", () => {
    const a = { id: 1, attachmentPath: "a", name: null };
    const b = { id: 2, attachmentPath: "b", name: null };
    expect(uniqueImages([{ images: [a, b] }, {}, { images: [a] }]).map((x) => x.id)).toEqual([1, 2]);
    expect(uniqueImages([])).toEqual([]);
  });
});
