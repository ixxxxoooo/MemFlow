// @author ygw
import { describe, expect, it } from "vitest";
import { APP_SHORTCUTS, shortcutAction } from "./appShortcuts";

const ev = (key: string, o: Partial<Parameters<typeof shortcutAction>[0]> = {}) => ({ key, metaKey: true, ctrlKey: false, shiftKey: false, altKey: false, defaultPrevented: false, ...o });

describe("shortcutAction", () => {
  it("⌘, 打开设置;⌘N 记一笔;⌘1/2/3 切换视图", () => {
    expect(shortcutAction(ev(","))).toBe("settings");
    expect(shortcutAction(ev("n"))).toBe("capture");
    expect(shortcutAction(ev("N"))).toBe("capture");
    expect(shortcutAction(ev("1"))).toBe("chat");
    expect(shortcutAction(ev("2"))).toBe("timeline");
    expect(shortcutAction(ev("3"))).toBe("docs");
  });
  it("Ctrl 同样生效(非 macOS)", () => {
    expect(shortcutAction(ev(",", { metaKey: false, ctrlKey: true }))).toBe("settings");
  });
  it("没有修饰键、带 Shift/Alt 或其他按键都不响应", () => {
    expect(shortcutAction(ev(",", { metaKey: false }))).toBeNull();
    expect(shortcutAction(ev(",", { shiftKey: true }))).toBeNull();
    expect(shortcutAction(ev("n", { altKey: true }))).toBeNull();
    expect(shortcutAction(ev("k"))).toBeNull();
  });
  it("事件已被处理(录制快捷键时)或输入法组字中不响应", () => {
    expect(shortcutAction(ev(",", { defaultPrevented: true }))).toBeNull();
    expect(shortcutAction(ev("n", { isComposing: true }))).toBeNull();
  });
  it("清单与动作一一对应", () => {
    expect(new Set(APP_SHORTCUTS.map((s) => s.action)).size).toBe(APP_SHORTCUTS.length);
  });
});
