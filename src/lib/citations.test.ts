// @author ygw
import { describe, expect, it } from "vitest";
import { linkCitations } from "./citations";

describe("linkCitations", () => {
  it("把合法角标转为链接", () => {
    expect(linkCitations("预算缩减 [1][2]", 3)).toBe("预算缩减 [1](cite:1)[2](cite:2)");
  });
  it("超出范围的角标保持原样", () => {
    expect(linkCitations("见 [9]", 3)).toBe("见 [9]");
  });
  it("代码块内不处理", () => {
    const t = "文字 [1]\n```mermaid\nA[1] --> B\n```";
    const r = linkCitations(t, 2);
    expect(r).toContain("A[1] --> B");
    expect(r).toContain("[1](cite:1)");
  });
  it("已是链接的不重复处理", () => {
    expect(linkCitations("[1](http://a.com)", 3)).toBe("[1](http://a.com)");
  });
});
