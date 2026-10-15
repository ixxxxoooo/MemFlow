// @author ixxxxoooo
// 「记一笔」:存入默认知识库、拖入文件作为附件、无内容不可保存、失败时保留草稿

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ captureSubmit: vi.fn(), stageFile: vi.fn() }));
vi.mock("../../lib/api", () => ({ api }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import { CAPTURE_FILES_EVENT, QuickCaptureDialog } from "./QuickCaptureDialog";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flush = async () => { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); };
const btn = (text: string) => [...document.body.querySelectorAll("button")].find((b) => b.textContent?.trim() === text) as HTMLButtonElement;
function setNative(el: HTMLTextAreaElement, v: string) {
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(el, v);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("QuickCaptureDialog", () => {
  let el: HTMLDivElement;
  let root: Root;
  const toast = vi.fn();
  const bumpData = vi.fn();
  beforeEach(() => {
    api.captureSubmit.mockReset();
    toast.mockReset();
    bumpData.mockReset();
    useKb.setState({ kbs: [{ id: 1, name: "默认知识库", description: "", createdAt: 0, itemCount: 0, documentCount: 0, chunkCount: 0 }] });
    useApp.setState({ captureOpen: false, toast, bumpData, refreshStatus: vi.fn(async () => {}) } as never);
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
    act(() => root.render(<QuickCaptureDialog />));
  });
  afterEach(() => { act(() => root.unmount()); el.remove(); document.body.innerHTML = ""; });

  it("关闭时不渲染;openCapture 后出现并提示存入默认库", () => {
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
    act(() => useApp.getState().openCapture());
    const dlg = document.body.querySelector('[role="dialog"]')!;
    expect(dlg.textContent).toContain("存入「默认知识库」");
  });

  it("没有内容时保存按钮禁用", () => {
    act(() => useApp.getState().openCapture());
    expect(btn("保存").disabled).toBe(true);
  });

  it("保存:提交到默认知识库(id=1)、提示、刷新并关闭", async () => {
    api.captureSubmit.mockResolvedValue({ created: [5], duplicates: [], skipped: [], skippedCount: 0, errors: [] });
    act(() => useApp.getState().openCapture());
    act(() => setNative(document.body.querySelector("textarea")!, "明天下午三点开会"));
    await act(async () => { btn("保存").click(); });
    await flush();
    expect(api.captureSubmit).toHaveBeenCalledWith("明天下午三点开会", [], 1);
    expect(toast).toHaveBeenCalledWith(expect.stringContaining("已交由 AI 归档"), "ok");
    expect(bumpData).toHaveBeenCalled();
    expect(useApp.getState().captureOpen).toBe(false);
  });

  it("窗口内拖入的文件作为附件一并提交", async () => {
    api.captureSubmit.mockResolvedValue({ created: [5], duplicates: [], skipped: [], skippedCount: 0, errors: [] });
    act(() => useApp.getState().openCapture());
    act(() => { window.dispatchEvent(new CustomEvent(CAPTURE_FILES_EVENT, { detail: ["/tmp/a.pdf"] })); });
    expect(document.body.textContent).toContain("a.pdf");
    await act(async () => { btn("保存").click(); });
    await flush();
    expect(api.captureSubmit).toHaveBeenCalledWith(null, ["/tmp/a.pdf"], 1);
  });

  it("保存失败:对话框保持打开、草稿保留并显示原因", async () => {
    api.captureSubmit.mockRejectedValue(new Error("磁盘已满"));
    act(() => useApp.getState().openCapture());
    act(() => setNative(document.body.querySelector("textarea")!, "草稿"));
    await act(async () => { btn("保存").click(); });
    await flush();
    expect(useApp.getState().captureOpen).toBe(true);
    expect(document.body.textContent).toContain("磁盘已满");
    expect(document.body.querySelector("textarea")!.value).toBe("草稿");
  });
});
