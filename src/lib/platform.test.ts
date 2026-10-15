// @author ixxxxoooo
import { describe, expect, it } from "vitest";
import { detectPlatform, shortcutLabel } from "./platform";
import { prettyAccelerator } from "./shortcut";

describe("detectPlatform", () => {
  it("按 User-Agent 区分三大平台", () => {
    expect(detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")).toBe("macos");
    expect(detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/120")).toBe("windows");
    expect(detectPlatform("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15")).toBe("linux");
  });
});

describe("按平台显示快捷键", () => {
  it("应用内快捷键", () => {
    expect(shortcutLabel("n", "macos")).toBe("⌘ N");
    expect(shortcutLabel(",", "windows")).toBe("Ctrl+,");
    expect(shortcutLabel("1", "linux")).toBe("Ctrl+1");
  });
  it("全局快捷键(CommandOrControl 在非 macOS 显示为 Ctrl)", () => {
    expect(prettyAccelerator("CommandOrControl+Shift+K", "macos")).toBe("⌘ ⇧ K");
    expect(prettyAccelerator("CommandOrControl+Shift+K", "windows")).toBe("Ctrl+Shift+K");
    expect(prettyAccelerator("CommandOrControl+Alt+Space", "linux")).toBe("Ctrl+Alt+Space");
  });
});
