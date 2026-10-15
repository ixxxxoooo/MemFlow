// @author ygw
import { describe, expect, it } from "vitest";
import { filterKbs, importTargetKb, mentionAt, normalizeScope, removeMention, scopeLabel, toggleScope } from "./kbScope";
import type { Kb } from "./types";

const kb = (id: number, name: string): Kb => ({ id, name, description: "", createdAt: 0, itemCount: 0 });
const KBS = [kb(1, "默认知识库"), kb(2, "工作"), kb(3, "读书笔记")];

describe("mentionAt", () => {
  it("行首与空白后的 @ 触发", () => {
    expect(mentionAt("@", 1)).toEqual({ start: 0, query: "" });
    expect(mentionAt("帮我查 @工", 6)).toEqual({ start: 4, query: "工" });
  });
  it("邮箱、已有空白间隔、光标不在词尾均不触发", () => {
    expect(mentionAt("a@b.com", 7)).toBeNull();
    expect(mentionAt("@工作 然后", 6)).toBeNull();
    expect(mentionAt("没有提及", 4)).toBeNull();
  });
  it("光标在文本中间时只看光标之前", () => {
    expect(mentionAt("@工作 预算", 3)).toEqual({ start: 0, query: "工作" });
  });
});

describe("removeMention", () => {
  it("删除 @词 并保留其余文字", () => {
    const m = mentionAt("请问 @工 预算", 5)!;
    expect(removeMention("请问 @工 预算", m, 5)).toEqual({ text: "请问 预算", caret: 3 });
  });
  it("行首提及删除后无残留", () => {
    const m = mentionAt("@读", 2)!;
    expect(removeMention("@读", m, 2)).toEqual({ text: "", caret: 0 });
  });
});

describe("范围工具", () => {
  it("按名称筛选且不区分大小写", () => {
    expect(filterKbs([kb(1, "Work"), kb(2, "读书")], "wo").map((k) => k.id)).toEqual([1]);
    expect(filterKbs(KBS, "").length).toBe(3);
  });
  it("规整范围:去重并丢弃已删除的库", () => {
    expect(normalizeScope([2, 2, 9, 3], KBS)).toEqual([2, 3]);
  });
  it("切换选中状态", () => {
    expect(toggleScope([1], 2)).toEqual([1, 2]);
    expect(toggleScope([1, 2], 1)).toEqual([2]);
  });
  it("展示文案:空为全部", () => {
    expect(scopeLabel([], KBS)).toBe("全部知识库");
    expect(scopeLabel([2, 3], KBS)).toBe("工作、读书笔记");
    expect(scopeLabel([99], KBS)).toBe("全部知识库");
  });
});

describe("importTargetKb", () => {
  it("当前选择优先,其次最近导入,最后默认库", () => {
    expect(importTargetKb(2, 3, KBS)).toBe(2);
    expect(importTargetKb(null, 3, KBS)).toBe(3);
    expect(importTargetKb(null, null, KBS)).toBe(1);
  });
  it("引用的库已被删除时回退", () => {
    expect(importTargetKb(9, 3, KBS)).toBe(3);
    expect(importTargetKb(9, 8, KBS)).toBe(1);
  });
});
