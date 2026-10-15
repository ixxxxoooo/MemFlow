// @author ygw
// 问答范围选择器:多选、「全部」互斥、单库时隐藏、范围持久化

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../lib/api", () => ({ api: {} }));

import { useChat } from "../../lib/chatStore";
import { useKb } from "../../lib/kbStore";
import type { Kb } from "../../lib/types";
import { ChatKbPicker } from "./ChatKbPicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const kb = (id: number, name: string): Kb => ({ id, name, description: "", createdAt: 0, itemCount: 0, documentCount: id, chunkCount: 0 });
const rows = () => [...document.body.querySelectorAll("button")].filter((b) => /全部知识库|工作|读书/.test(b.textContent ?? "") && !b.hasAttribute("data-testid"));

describe("ChatKbPicker", () => {
  let el: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    localStorage.clear();
    useKb.setState({ kbs: [kb(1, "默认知识库"), kb(2, "工作"), kb(3, "读书笔记")] });
    useChat.setState({ kbIds: [], busy: false });
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
  });
  afterEach(() => { act(() => root.unmount()); el.remove(); document.body.innerHTML = ""; });

  it("只有一个知识库时不显示", () => {
    useKb.setState({ kbs: [kb(1, "默认知识库")] });
    act(() => root.render(<ChatKbPicker />));
    expect(el.querySelector('[data-testid="chat-kb-picker"]')).toBeNull();
  });

  it("默认显示「全部知识库」,点开可多选,选「全部」清空其他", () => {
    act(() => root.render(<ChatKbPicker />));
    const trigger = el.querySelector('[data-testid="chat-kb-picker"]') as HTMLButtonElement;
    expect(trigger.textContent).toContain("全部知识库");
    act(() => trigger.click());
    const find = (t: string) => rows().find((b) => b.textContent?.includes(t))!;
    act(() => find("工作").click());
    act(() => find("读书笔记").click());
    expect(useChat.getState().kbIds).toEqual([2, 3]);
    expect(trigger.textContent).toContain("工作、读书笔记");
    // 范围持久化:新对话与重启后沿用
    expect(JSON.parse(localStorage.getItem("memflow.chatScope")!)).toEqual([2, 3]);
    act(() => find("全部知识库").click());
    expect(useChat.getState().kbIds).toEqual([]);
  });

  it("再次点击已选库可取消", () => {
    useChat.setState({ kbIds: [2] });
    act(() => root.render(<ChatKbPicker />));
    act(() => (el.querySelector('[data-testid="chat-kb-picker"]') as HTMLButtonElement).click());
    act(() => rows().find((b) => b.textContent?.includes("工作"))!.click());
    expect(useChat.getState().kbIds).toEqual([]);
  });

  it("回答生成中不可修改范围", () => {
    useChat.setState({ busy: true });
    act(() => root.render(<ChatKbPicker />));
    expect((el.querySelector('[data-testid="chat-kb-picker"]') as HTMLButtonElement).disabled).toBe(true);
  });
});
