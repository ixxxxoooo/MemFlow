// @author ixxxxoooo
// 知识档案库两级结构:列表 → 库内;新建/编辑/删除;库内导入固定到当前库;格式提示

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  listKbs: vi.fn(),
  createKb: vi.fn(),
  renameKb: vi.fn(),
  deleteKb: vi.fn(),
  listItems: vi.fn(),
  captureSubmit: vi.fn(),
}));
const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("../../lib/api", () => ({ api }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);

import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import type { Kb } from "../../lib/types";
import { DocsView } from "./DocsView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flush = async () => { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); };
const kb = (id: number, name: string, description = ""): Kb => ({ id, name, description, createdAt: 1_700_000_000_000, itemCount: id, documentCount: id * 2, chunkCount: id * 10 });
const button = (el: ParentNode, text: string) => [...el.querySelectorAll("button")].find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;
function setNative(el: HTMLInputElement | HTMLTextAreaElement, v: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, v);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("知识档案库两级结构", () => {
  let el: HTMLDivElement;
  let root: Root;
  const toast = vi.fn();
  beforeEach(() => {
    Object.values(api).forEach((f) => f.mockReset());
    dialog.open.mockReset();
    toast.mockReset();
    const kbs = [kb(1, "默认知识库", "默认放这里"), kb(2, "工作")];
    api.listKbs.mockResolvedValue(kbs);
    api.listItems.mockResolvedValue([]);
    api.captureSubmit.mockResolvedValue({ created: [10], duplicates: [], skipped: [], skippedCount: 0, errors: [], linkedImages: 0 });
    useKb.setState({ kbs });
    useApp.setState({ docsKb: null, toast, bumpData: vi.fn(), refreshStatus: vi.fn(async () => {}), status: null, view: "docs" } as never);
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
  });
  afterEach(() => { act(() => root.unmount()); el.remove(); document.body.innerHTML = ""; });

  it("先显示知识库列表(名称、描述、文档数),不显示文档表", () => {
    act(() => root.render(<DocsView />));
    expect(el.querySelector('[data-testid="kb-grid"]')).toBeTruthy();
    expect(el.textContent).toContain("默认知识库");
    expect(el.textContent).toContain("默认放这里");
    expect(el.textContent).toContain("4 个文档");
    expect(el.textContent).not.toContain("导入文件夹");
    expect(api.listItems).not.toHaveBeenCalled();
  });

  it("默认库没有删除按钮,其余库有", () => {
    act(() => root.render(<DocsView />));
    expect(el.querySelector('[aria-label="删除 默认知识库"]')).toBeNull();
    expect(el.querySelector('[aria-label="删除 工作"]')).toBeTruthy();
  });

  it("点击知识库进入库内,按 library 视图加载并显示面包屑与格式提示", async () => {
    act(() => root.render(<DocsView />));
    act(() => (el.querySelector('[data-testid="kb-card-2"]') as HTMLElement).click());
    await flush();
    expect(useApp.getState().docsKb).toBe(2);
    expect(api.listItems).toHaveBeenCalledWith(expect.objectContaining({ view: "library", kbId: 2 }));
    expect(el.textContent).toContain("工作");
    expect(el.querySelector('[data-testid="formats-hint"]')?.textContent).toContain("PDF · Word · PPT");
    // 返回列表
    act(() => button(el, "知识档案库")!.click());
    expect(useApp.getState().docsKb).toBeNull();
  });

  it("库内导入文件夹固定导入到当前打开的库", async () => {
    useApp.setState({ docsKb: 2 });
    dialog.open.mockResolvedValue("/tmp/资料");
    act(() => root.render(<DocsView />));
    await flush();
    await act(async () => { button(el, "导入文件夹")!.click(); });
    await flush();
    expect(dialog.open).toHaveBeenCalledWith({ directory: true, multiple: false });
    expect(api.captureSubmit).toHaveBeenCalledWith(null, ["/tmp/资料"], 2);
  });

  it("导入文件的对话框带上全部支持的扩展名", async () => {
    useApp.setState({ docsKb: 1 });
    dialog.open.mockResolvedValue(null);
    act(() => root.render(<DocsView />));
    await flush();
    await act(async () => { ([...el.querySelectorAll("button")].find((b) => b.textContent?.trim() === "导入文件") as HTMLButtonElement).click(); });
    const arg = dialog.open.mock.calls[0][0] as { filters: { extensions: string[] }[] };
    expect(arg.filters[0].extensions).toEqual(expect.arrayContaining(["pdf", "docx", "pptx", "xlsx", "csv", "html", "epub", "rtf", "md", "txt", "png"]));
  });

  it("新建知识库:填写名称与描述后提交", async () => {
    api.createKb.mockResolvedValue(kb(3, "旅行"));
    act(() => root.render(<DocsView />));
    act(() => button(el, "新建知识库")!.click());
    const dlg = document.body.querySelector('[role="dialog"]')!;
    act(() => setNative(dlg.querySelector("input")!, "旅行"));
    act(() => setNative(dlg.querySelector("textarea")!, "出行攻略"));
    await act(async () => { button(dlg, "保存")!.click(); });
    await flush();
    expect(api.createKb).toHaveBeenCalledWith("旅行", "出行攻略");
  });

  it("编辑知识库:提交名称与描述", async () => {
    api.renameKb.mockResolvedValue(undefined);
    act(() => root.render(<DocsView />));
    act(() => (el.querySelector('[aria-label="编辑 工作"]') as HTMLButtonElement).click());
    const dlg = document.body.querySelector('[role="dialog"]')!;
    act(() => setNative(dlg.querySelector("textarea")!, "公司事务"));
    await act(async () => { button(dlg, "保存")!.click(); });
    await flush();
    expect(api.renameKb).toHaveBeenCalledWith(2, "工作", "公司事务");
  });

  it("跨库搜索:输入关键字后列出各库的匹配资料并标注所属库", async () => {
    api.listItems.mockResolvedValue([{ id: 7, kind: "document", title: "预算.pdf", originalName: "预算.pdf", kbId: 2, createdAt: 1_700_000_000_000 }]);
    act(() => root.render(<DocsView />));
    act(() => setNative(el.querySelector("input")!, "预算"));
    await act(async () => { await new Promise((r) => setTimeout(r, 300)); });
    expect(api.listItems).toHaveBeenCalledWith(expect.objectContaining({ view: "library", search: "预算" }));
    const res = el.querySelector('[data-testid="kb-search-results"]')!;
    expect(res.textContent).toContain("预算.pdf");
    expect(res.textContent).toContain("工作");
  });
});
