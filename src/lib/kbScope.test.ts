// @author ixxxxoooo
import { describe, expect, it } from "vitest";
import { dropTargetKb, normalizeScope, scopeLabel, toggleScope } from "./kbScope";
import type { Kb } from "./types";

const kb = (id: number, name: string): Kb => ({ id, name, description: "", createdAt: 0, itemCount: 0, documentCount: 0, chunkCount: 0 });
const KBS = [kb(1, "默认知识库"), kb(2, "工作"), kb(3, "读书笔记"), kb(4, "旅行")];

describe("normalizeScope / toggleScope", () => {
  it("去掉不存在的库并去重", () => {
    expect(normalizeScope([2, 9, 2, 3], KBS)).toEqual([2, 3]);
    expect(normalizeScope([], KBS)).toEqual([]);
  });
  it("切换选中状态", () => {
    expect(toggleScope([2], 3)).toEqual([2, 3]);
    expect(toggleScope([2, 3], 2)).toEqual([3]);
  });
});

describe("scopeLabel", () => {
  it("空范围 = 全部", () => expect(scopeLabel([], KBS)).toBe("全部知识库"));
  it("一到两个库直接列名,更多则折叠", () => {
    expect(scopeLabel([2], KBS)).toBe("工作");
    expect(scopeLabel([2, 3], KBS)).toBe("工作、读书笔记");
    expect(scopeLabel([2, 3, 4], KBS)).toBe("工作、读书笔记 等 3 个");
  });
  it("已删除的库不展示", () => expect(scopeLabel([9], KBS)).toBe("全部知识库"));
});

describe("dropTargetKb", () => {
  it("在档案库里打开了某个库:导入该库", () => expect(dropTargetKb(3, true, KBS)).toBe(3));
  it("不在档案库页或没打开库:导入默认库", () => {
    expect(dropTargetKb(3, false, KBS)).toBe(1);
    expect(dropTargetKb(null, true, KBS)).toBe(1);
  });
  it("打开的库已被删除:回到默认库", () => expect(dropTargetKb(9, true, KBS)).toBe(1));
});
