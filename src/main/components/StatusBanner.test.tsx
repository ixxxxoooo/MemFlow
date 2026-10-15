// @author ixxxxoooo
// 顶部提示条:可关闭(并记住)、钥匙串读取失败时提示重新授权而不是"未配置"

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  retryKeyAccess: vi.fn(),
  ollamaStart: vi.fn(),
  reindex: vi.fn(),
}));
vi.mock("../../lib/api", () => ({ api }));

import { useApp } from "../../lib/store";
import { StatusBanner } from "./StatusBanner";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flush = async () => { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); };
const button = (el: HTMLElement, text: string) => [...el.querySelectorAll("button")].find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;

/** 构造"已选云端模型但没有密钥"的状态 */
function cloudStatus(extra: Record<string, unknown> = {}) {
  return {
    ollamaRunning: true,
    models: [{ role: "文本模型(云端)", name: "deepseek-v4", installed: false, required: true, local: false }],
    stats: { staleChunks: 0, items: 0 },
    queue: { running: false, pending: 0 },
    keyErrors: [],
    ...extra,
  } as never;
}

describe("StatusBanner", () => {
  let el: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    Object.values(api).forEach((f) => f.mockReset());
    localStorage.clear();
    useApp.setState({ status: cloudStatus(), toast: vi.fn(), refreshStatus: vi.fn(async () => {}) });
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
  });
  afterEach(() => { act(() => root.unmount()); el.remove(); });

  it("云端未配置密钥的提示右侧有关闭图标,点击后消失且重新挂载仍保持关闭", () => {
    act(() => root.render(<StatusBanner />));
    expect(el.textContent).toContain("尚未配置 API Key");
    const close = el.querySelector('button[aria-label="关闭提示"]') as HTMLButtonElement;
    expect(close).toBeTruthy();
    act(() => close.click());
    expect(el.textContent).not.toContain("尚未配置 API Key");

    // 模拟重启:卸载后重新挂载
    act(() => root.unmount());
    root = createRoot(el);
    act(() => root.render(<StatusBanner />));
    expect(el.textContent).not.toContain("尚未配置 API Key");
  });

  it("Ollama 未运行:仍需要本地模型时提示启动;全部选云端时不提示", () => {
    const cloud = { role: "文本模型(云端)", name: "m", installed: true, required: true, local: false };
    const localEmbed = { role: "向量模型", name: "bge-m3", installed: true, required: true, local: true };
    useApp.setState({ status: cloudStatus({ ollamaRunning: false, models: [cloud, localEmbed] }) });
    act(() => root.render(<StatusBanner />));
    expect(el.textContent).toContain("未检测到 Ollama");
    act(() => {
      useApp.setState({ status: cloudStatus({ ollamaRunning: false, models: [cloud, { ...localEmbed, local: false, role: "向量模型(云端)" }] }) });
    });
    expect(el.textContent).not.toContain("未检测到 Ollama");
  });

  it("换了一个云端模型,会重新提示", () => {
    act(() => root.render(<StatusBanner />));
    act(() => (el.querySelector('button[aria-label="关闭提示"]') as HTMLButtonElement).click());
    expect(el.textContent).toBe("");
    act(() => {
      useApp.setState({ status: cloudStatus({ models: [{ role: "文本模型(云端)", name: "gpt-x", installed: false, required: true, local: false }] }) });
    });
    expect(el.textContent).toContain("gpt-x");
  });

  it("密钥已保存但钥匙串读取失败:提示重新授权,点击后调用重试", async () => {
    api.retryKeyAccess.mockResolvedValue(undefined);
    useApp.setState({
      status: cloudStatus({
        models: [{ role: "文本模型(云端)", name: "deepseek-v4", installed: true, required: true, local: false }],
        keyErrors: [{ profile: "default", message: "钥匙串授权被拒绝" }],
      }),
    });
    act(() => root.render(<StatusBanner />));
    expect(el.textContent).toContain("已保存,但读取失败");
    expect(el.textContent).not.toContain("尚未配置");
    await act(async () => { button(el, "重新授权")!.click(); });
    await flush();
    expect(api.retryKeyAccess).toHaveBeenCalledWith(null);
  });
});
