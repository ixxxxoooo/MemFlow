// @author ygw
// AI 整理对话框:本地模型直接生成、云端先提示隐私、确认时提交用户修改后的内容

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  organizePropose: vi.fn(),
  organizeApply: vi.fn(),
  organizeMergePropose: vi.fn(),
  organizeMergeApply: vi.fn(),
  organizeSuggest: vi.fn(),
  listChatModels: vi.fn(),
  appStatus: vi.fn(),
}));
vi.mock("../../lib/api", () => ({ api }));

import { useChat } from "../../lib/chatStore";
import { useOrganize } from "../../lib/organize";
import { useApp } from "../../lib/store";
import { OrganizeDialog } from "./OrganizeDialog";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flush = async () => { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); };
const brief = (id: number) => ({ id, kbId: 1, title: null, preview: `碎片${id}`, createdAt: 1_700_000_000_000 + id });
const single = { itemId: 7, baseUpdatedAt: 99, originalTitle: null, originalContent: "原文内容", title: "新标题", content: "整理后的内容", notes: ["日期不明"], warnings: ["整理后丢失了原文中的数字:20"], model: "qwen", cloud: false };
const merge = { ids: [1, 2], sources: [brief(1), brief(2)], title: "主题", content: "结论[1][2]", notes: [], warnings: [], model: "qwen", cloud: false };

function setNative(el: HTMLTextAreaElement | HTMLInputElement, v: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, v);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}
const button = (el: HTMLElement, text: string) => [...el.querySelectorAll("button")].find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;

describe("OrganizeDialog", () => {
  let el: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    Object.values(api).forEach((f) => f.mockReset());
    api.listChatModels.mockResolvedValue([]);
    api.appStatus.mockResolvedValue(null);
    useOrganize.setState({ dialog: null });
    useChat.setState({ models: [], modelRef: null });
    useApp.setState({ status: { settings: { llmProvider: "ollama", textModel: "qwen2.5:7b", llmModel: "" } } as never });
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
  });
  afterEach(() => { act(() => root.unmount()); el.remove(); });

  it("单条整理:展示原文与整理结果及警告,确认时提交编辑后的内容", async () => {
    api.organizePropose.mockResolvedValue(single);
    api.organizeApply.mockResolvedValue(undefined);
    act(() => { root.render(<OrganizeDialog />); useOrganize.getState().openSingle(7); });
    await flush();
    expect(api.organizePropose).toHaveBeenCalledWith(7, null);
    expect(el.textContent).toContain("原文内容");
    expect(el.textContent).toContain("整理后丢失了原文中的数字:20");
    expect(el.textContent).toContain("日期不明");
    const ta = el.querySelector("textarea")!;
    expect(ta.value).toBe("整理后的内容");
    act(() => setNative(ta, "我改过的内容"));
    await act(async () => { button(el, "覆盖原文")!.click(); });
    expect(api.organizeApply).toHaveBeenCalledWith(7, "新标题", "我改过的内容", 99);
    expect(useOrganize.getState().dialog).toBeNull();
  });

  it("云端模型:先显示隐私提示,点击继续后才发起请求", async () => {
    api.organizeMergePropose.mockResolvedValue(merge);
    useApp.setState({ status: { settings: { llmProvider: "openai", textModel: "q", llmModel: "deepseek-chat" } } as never });
    act(() => { root.render(<OrganizeDialog />); useOrganize.getState().openMerge([1, 2]); });
    await flush();
    expect(api.organizeMergePropose).not.toHaveBeenCalled();
    expect(el.textContent).toContain("将把 2 条笔记的原文发送给云端模型");
    await act(async () => { button(el, "我了解")!.click(); });
    await flush();
    expect(api.organizeMergePropose).toHaveBeenCalledWith([1, 2], null);
    expect(el.textContent).toContain("碎片1");
  });

  it("归并预览:来源带编号,创建时传入来源顺序与编辑后的内容", async () => {
    api.organizeMergePropose.mockResolvedValue(merge);
    api.organizeMergeApply.mockResolvedValue(55);
    act(() => { root.render(<OrganizeDialog />); useOrganize.getState().openMerge([2, 1]); });
    await flush();
    expect(el.textContent).toContain("碎片1");
    expect(el.textContent).toContain("碎片2");
    await act(async () => { button(el, "创建主题笔记")!.click(); });
    expect(api.organizeMergeApply).toHaveBeenCalledWith([1, 2], "主题", "结论[1][2]");
    expect(useApp.getState().drawer?.itemId).toBe(55);
  });

  it("生成失败:显示错误并可重试", async () => {
    api.organizePropose.mockRejectedValueOnce(new Error("模型没有返回整理结果")).mockResolvedValueOnce(single);
    act(() => { root.render(<OrganizeDialog />); useOrganize.getState().openSingle(7); });
    await flush();
    expect(el.textContent).toContain("模型没有返回整理结果");
    await act(async () => { button(el, "重试")!.click(); });
    await flush();
    expect(el.textContent).toContain("原文内容");
    expect(api.organizePropose).toHaveBeenCalledTimes(2);
  });

  it("碎片建议:列出分组,点击预览归并进入归并流程", async () => {
    api.organizeSuggest.mockResolvedValue([{ ids: [1, 2, 3], items: [brief(1), brief(2), brief(3)], avgSimilarity: 0.83 }]);
    api.organizeMergePropose.mockResolvedValue(merge);
    act(() => { root.render(<OrganizeDialog />); useOrganize.getState().openSuggest(); });
    await flush();
    expect(el.textContent).toContain("3 条相近碎片");
    expect(el.textContent).toContain("83%");
    await act(async () => { button(el, "预览归并")!.click(); });
    await flush();
    expect(api.organizeMergePropose).toHaveBeenCalledWith([1, 2, 3], null);
    expect(button(el, "返回列表")).toBeTruthy();
  });
});
