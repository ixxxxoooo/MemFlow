// @author ygw
import { describe, expect, it } from "vitest";
import { skipDetailLine, summarizeCapture } from "./importSummary";
import type { CaptureResult } from "./types";

const base: CaptureResult = { created: [], duplicates: [], errors: [], skipped: [], skippedCount: 0 };

describe("summarizeCapture", () => {
  it("导入、重复、跳过一起汇总", () => {
    const s = summarizeCapture({ ...base, created: [1, 2, 3], duplicates: [9], skipped: ["不支持的类型(.zip) ×2"], skippedCount: 2 }, "工作");
    expect(s.text).toBe("导入 3 项到「工作」、重复 1、跳过 2");
    expect(s.kind).toBe("ok");
    expect(s.detail).toEqual(["不支持的类型(.zip) ×2"]);
  });
  it("有失败时为 err", () => {
    expect(summarizeCapture({ ...base, errors: ["x"] }).kind).toBe("err");
  });
  it("什么都没导入时给出提示", () => {
    const s = summarizeCapture(base);
    expect(s.text).toBe("没有可导入的内容");
    expect(s.kind).toBe("info");
  });
  it("明细过多时截断", () => {
    expect(skipDetailLine(["a ×1", "b ×2", "c ×3", "d ×4"], 3)).toBe("a ×1;b ×2;c ×3 等 4 类");
    expect(skipDetailLine([])).toBe("");
  });
});
