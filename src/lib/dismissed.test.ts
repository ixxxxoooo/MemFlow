// @author ixxxxoooo
import { beforeEach, describe, expect, it } from "vitest";
import { loadDismissed, saveDismissed } from "./dismissed";

describe("提示条关闭记忆", () => {
  beforeEach(() => localStorage.clear());

  it("保存后可读回", () => {
    saveDismissed(new Set(["cloud-no-key:m1"]));
    expect(loadDismissed().has("cloud-no-key:m1")).toBe(true);
  });

  it("内容损坏时视为空", () => {
    localStorage.setItem("memflow.dismissedBanners", "{坏的");
    expect(loadDismissed().size).toBe(0);
    localStorage.setItem("memflow.dismissedBanners", JSON.stringify({ a: 1 }));
    expect(loadDismissed().size).toBe(0);
  });

  it("只保留最近 50 条", () => {
    saveDismissed(new Set(Array.from({ length: 60 }, (_, i) => `k${i}`)));
    const s = loadDismissed();
    expect(s.size).toBe(50);
    expect(s.has("k59")).toBe(true);
    expect(s.has("k0")).toBe(false);
  });
});
