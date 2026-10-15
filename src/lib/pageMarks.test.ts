import { describe, expect, it } from "vitest";
import { pageLabel, stripPageMarks } from "./pageMarks";

describe("pageMarks", () => {
  it("去掉页标记并保留正文", () => {
    expect(stripPageMarks("\uE000\n甲\n\n\uE000\n乙")).toBe("甲\n\n乙");
  });
  it("没有标记时原样返回", () => {
    expect(stripPageMarks("普通文本")).toBe("普通文本");
  });
  it("页码标签", () => {
    expect(pageLabel(3)).toBe(" · p.3");
    expect(pageLabel(null)).toBe("");
    expect(pageLabel(undefined)).toBe("");
  });
});
