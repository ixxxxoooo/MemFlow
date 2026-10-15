// @author ygw
import { describe, expect, it } from "vitest";
import { wheelIntent } from "./panzoom";

const base = { deltaX: 0, deltaY: 0, ctrlKey: false, metaKey: false };

describe("wheelIntent", () => {
  it("普通滚轮平移,方向与内容滚动一致", () => {
    expect(wheelIntent({ ...base, deltaY: 40, deltaX: 10 })).toEqual({ type: "pan", dx: -10, dy: -40 });
  });

  it("按住 Ctrl 或 ⌘ 缩放:向上放大,向下缩小", () => {
    const up = wheelIntent({ ...base, ctrlKey: true, deltaY: -20 });
    const down = wheelIntent({ ...base, metaKey: true, deltaY: 20 });
    expect(up.type).toBe("zoom");
    expect(up.type === "zoom" && up.factor).toBeGreaterThan(1);
    expect(down.type === "zoom" && down.factor).toBeLessThan(1);
  });

  it("缩放倍率被限幅,鼠标大步长也不会突变", () => {
    const big = wheelIntent({ ...base, ctrlKey: true, deltaY: -5000 });
    expect(big.type === "zoom" && big.factor).toBeLessThan(1.4);
  });
});
