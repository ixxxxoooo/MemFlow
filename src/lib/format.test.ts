// @author ixxxxoooo
import { describe, expect, it } from "vitest";
import { fmtDuration } from "./format";

describe("fmtDuration", () => {
  it("分档格式化耗时", () => {
    expect(fmtDuration(320)).toBe("320ms");
    expect(fmtDuration(1234)).toBe("1.2s");
    expect(fmtDuration(65000)).toBe("1分5秒");
  });
});
