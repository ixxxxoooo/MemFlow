// @author ixxxxoooo
// API Key 眼睛图标:输入内容可切换明文;已保存时取回明文并自动隐藏;失败时提示

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ revealApiKey: vi.fn() }));
vi.mock("../../lib/api", () => ({ api }));

import { useApp } from "../../lib/store";
import { SecretInput } from "./SecretInput";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flush = async () => { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); };

describe("SecretInput", () => {
  let el: HTMLDivElement;
  let root: Root;
  const toast = vi.fn();
  const input = () => el.querySelector("input")!;
  const eye = () => el.querySelector("button")!;
  const render = (p: Partial<Parameters<typeof SecretInput>[0]> = {}) =>
    act(() => root.render(<SecretInput value="" onChange={() => {}} saved={false} {...p} />));

  beforeEach(() => {
    api.revealApiKey.mockReset();
    toast.mockReset();
    useApp.setState({ toast } as never);
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
  });
  afterEach(() => { vi.useRealTimers(); act(() => root.unmount()); el.remove(); });

  it("默认以密码形式显示;没有内容也没有保存时眼睛不可点", () => {
    render();
    expect(input().type).toBe("password");
    expect(eye().disabled).toBe(true);
  });

  it("输入框里有内容:点击眼睛切换明文,再点隐藏,不请求后端", () => {
    render({ value: "sk-abc" });
    act(() => eye().click());
    expect(input().type).toBe("text");
    expect(input().value).toBe("sk-abc");
    act(() => eye().click());
    expect(input().type).toBe("password");
    expect(api.revealApiKey).not.toHaveBeenCalled();
  });

  it("已保存且输入为空:点击后取回明文(只读展示),按档案标识请求", async () => {
    api.revealApiKey.mockResolvedValue("sk-saved-123");
    render({ saved: true, profileId: "p1" });
    await act(async () => { eye().click(); });
    await flush();
    expect(api.revealApiKey).toHaveBeenCalledWith("p1");
    expect(input().type).toBe("text");
    expect(input().value).toBe("sk-saved-123");
    expect(input().readOnly).toBe(true);
    act(() => eye().click());
    expect(input().value).toBe("");
    expect(input().type).toBe("password");
  });

  it("取回的明文 10 秒后自动隐藏", async () => {
    vi.useFakeTimers();
    api.revealApiKey.mockResolvedValue("sk-saved-123");
    render({ saved: true });
    await act(async () => { eye().click(); await Promise.resolve(); });
    expect(input().value).toBe("sk-saved-123");
    await act(async () => { vi.advanceTimersByTime(10_000); });
    expect(input().value).toBe("");
    expect(input().type).toBe("password");
  });

  it("取回失败:提示原因,不展示", async () => {
    api.revealApiKey.mockRejectedValue(new Error("钥匙串授权被拒绝"));
    render({ saved: true });
    await act(async () => { eye().click(); });
    await flush();
    expect(toast).toHaveBeenCalledWith("钥匙串授权被拒绝", "err");
    expect(input().value).toBe("");
  });
});
