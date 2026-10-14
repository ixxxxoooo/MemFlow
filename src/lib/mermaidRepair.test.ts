// @author ygw
import { describe, expect, it, vi } from "vitest";
import { normalizeMermaid, renderWithRepair } from "./mermaidRepair";

describe("normalizeMermaid", () => {
  it("去掉围栏与语言标记", () => {
    expect(normalizeMermaid("```mermaid\nflowchart TD\nA-->B\n```")).toBe("flowchart TD\nA-->B");
    expect(normalizeMermaid("mermaid\nflowchart TD")).toBe("flowchart TD");
    expect(normalizeMermaid("  graph LR  ")).toBe("graph LR");
  });
});

describe("renderWithRepair", () => {
  it("一次成功不触发修复", async () => {
    const fix = vi.fn();
    const r = await renderWithRepair("graph TD", async () => "<svg/>", fix);
    expect(r.ok).toBe(true);
    expect(r.repairs).toBe(0);
    expect(fix).not.toHaveBeenCalled();
  });

  it("修复一次后成功", async () => {
    const render = vi.fn(async (c: string) => {
      if (c.includes("bad")) throw new Error("语法错误");
      return "<svg>ok</svg>";
    });
    const fix = vi.fn(async () => "graph TD\nA-->B");
    const r = await renderWithRepair("bad code", render, fix);
    expect(r.ok).toBe(true);
    expect(r.repairs).toBe(1);
    expect(r.code).toBe("graph TD\nA-->B");
  });

  it("最多修复两次后放弃并返回错误", async () => {
    const fix = vi.fn(async (c: string) => c + "!");
    const r = await renderWithRepair("x", async () => { throw new Error("boom"); }, fix, 2);
    expect(r.ok).toBe(false);
    expect(fix).toHaveBeenCalledTimes(2);
    expect(r.error).toBe("boom");
  });

  it("修复函数本身失败时安全返回", async () => {
    const r = await renderWithRepair("x", async () => { throw new Error("e1"); }, async () => { throw new Error("模型不可用"); });
    expect(r.ok).toBe(false);
    expect(r.error).toContain("模型不可用");
  });
});
