// @author ixxxxoooo
import { describe, expect, it } from "vitest";
import { isFenceClosed } from "./markdownFence";

describe("isFenceClosed", () => {
  it("写完的块视为闭合", () => {
    expect(isFenceClosed("```mermaid\nflowchart TD\nA-->B\n```")).toBe(true);
    expect(isFenceClosed("```mermaid\nflowchart TD\nA-->B\n```\n")).toBe(true);
  });

  it("流式中途的半截块视为未闭合", () => {
    expect(isFenceClosed("```mermaid\nflowchart TD\nA-->")).toBe(false);
    expect(isFenceClosed("```mermaid")).toBe(false);
  });

  it("末行不是纯围栏时未闭合", () => {
    expect(isFenceClosed("```mermaid\nA-->B ```")).toBe(false);
  });

  it("支持波浪线与更长的围栏", () => {
    expect(isFenceClosed("~~~\nx\n~~~")).toBe(true);
    expect(isFenceClosed("````\nx\n```")).toBe(false);
    expect(isFenceClosed("````\nx\n````")).toBe(true);
  });
});
