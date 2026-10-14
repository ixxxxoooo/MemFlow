// @author ygw
import { describe, expect, it } from "vitest";
import { docExt, editModeOf, renderAsMarkdown } from "./docKind";

describe("文档类型", () => {
  it("扩展名提取", () => {
    expect(docExt("A.B.MD")).toBe("md");
    expect(docExt("noext")).toBe("");
    expect(docExt(null)).toBe("");
  });
  it("编辑方式", () => {
    expect(editModeOf("a.md")).toBe("wysiwyg");
    expect(editModeOf("a.txt")).toBe("plain");
    expect(editModeOf("a.pdf")).toBe("none");
    expect(editModeOf("a.docx")).toBe("none");
  });
  it("渲染方式", () => {
    expect(renderAsMarkdown("a.docx")).toBe(true);
    expect(renderAsMarkdown("a.pdf")).toBe(false);
  });
});
