// @author ixxxxoooo
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { ALL_EXTENSIONS, DOC_EXTENSIONS, IMAGE_FORMAT } from "./formats";

/** 取出后端 classify 里某一类的扩展名 */
function rustExts(kind: "Image" | "Document"): string[] {
  const src = readFileSync(resolve(process.cwd(), "src-tauri/src/extract.rs"), "utf8");
  const fn = src.slice(src.indexOf("pub fn classify"), src.indexOf("pub fn ext_lower"));
  const arm = new RegExp(`((?:\\s*\\|?\\s*"[a-z0-9]+")+)\\s*=>\\s*FileKind::${kind}`).exec(fn);
  if (!arm) throw new Error("未能解析后端 classify");
  return [...arm[1].matchAll(/"([a-z0-9]+)"/g)].map((m) => m[1]);
}

describe("支持格式与后端一致", () => {
  it("文档扩展名完全一致", () => {
    expect([...DOC_EXTENSIONS].sort()).toEqual(rustExts("Document").sort());
  });
  it("图片扩展名完全一致", () => {
    expect([...IMAGE_FORMAT.exts].sort()).toEqual(rustExts("Image").sort());
  });
  it("扩展名无重复", () => {
    expect(new Set(ALL_EXTENSIONS).size).toBe(ALL_EXTENSIONS.length);
  });
});
