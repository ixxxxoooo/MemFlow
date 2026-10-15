// @author ygw
// 向量/视觉模型来源选择:本地显示模型输入行;云端显示档案、模型名与测试;切云端需隐私确认

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ testCloudModel: vi.fn() }));
const confirm = vi.hoisted(() => ({ confirmDialog: vi.fn() }));
vi.mock("../../lib/api", () => ({ api }));
vi.mock("../../lib/confirm", () => confirm);

import { useApp } from "../../lib/store";
import type { Settings } from "../../lib/types";
import { ModelSourceRows } from "./ModelSourceRows";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flush = async () => { await act(async () => { await new Promise((r) => setTimeout(r, 0)); }); };

const base = {
  embedProvider: "ollama", embedProfile: "default", embedCloudModel: "",
  visionProvider: "ollama", visionProfile: "default", visionCloudModel: "",
  llmProfiles: [{ id: "p1", name: "硅基流动", preset: "custom", baseUrl: "https://x/v1", model: "m" }],
} as unknown as Settings;

describe("ModelSourceRows", () => {
  let el: HTMLDivElement;
  let root: Root;
  const toast = vi.fn();
  const set = vi.fn();
  const sel = (label: string) => el.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!;
  const change = async (s: HTMLSelectElement, v: string) => {
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
      setter.call(s, v);
      s.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await flush();
  };
  const render = (form: Settings, kind: "embed" | "vision" = "embed", keyed = ["default"], pending = "") =>
    act(() => root.render(
      <ModelSourceRows kind={kind} form={form} set={set} keyedProfiles={keyed} pendingDefaultKey={pending}>
        <div data-testid="local-row">本地模型输入行</div>
      </ModelSourceRows>,
    ));

  beforeEach(() => {
    api.testCloudModel.mockReset();
    confirm.confirmDialog.mockReset();
    set.mockReset();
    toast.mockReset();
    useApp.setState({ toast } as never);
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
  });
  afterEach(() => { act(() => root.unmount()); el.remove(); });

  it("本地来源:显示本地模型输入行,不显示云端档案", () => {
    render(base);
    expect(el.querySelector('[data-testid="local-row"]')).not.toBeNull();
    expect(sel("向量模型云端档案")).toBeNull();
  });

  it("切到云端:先隐私确认,同意后写入来源;拒绝则不变", async () => {
    confirm.confirmDialog.mockResolvedValueOnce(false);
    render(base);
    await change(sel("向量模型来源"), "openai_compat");
    expect(set).not.toHaveBeenCalled();
    confirm.confirmDialog.mockResolvedValueOnce(true);
    await change(sel("向量模型来源"), "openai_compat");
    expect(confirm.confirmDialog).toHaveBeenCalledTimes(2);
    expect(confirm.confirmDialog.mock.calls[0][0]).toContain("文本片段");
    expect(set).toHaveBeenCalledWith("embedProvider", "openai_compat");
  });

  it("视觉的隐私提示说明图片会被发送", async () => {
    confirm.confirmDialog.mockResolvedValueOnce(true);
    render(base, "vision");
    await change(sel("视觉模型来源"), "openai_compat");
    expect(confirm.confirmDialog.mock.calls[0][0]).toContain("图片");
    expect(set).toHaveBeenCalledWith("visionProvider", "openai_compat");
  });

  it("云端来源:显示档案(含未保存密钥提示)与模型名,隐藏本地输入行", () => {
    render({ ...base, embedProvider: "openai_compat", embedCloudModel: "emb" }, "embed", ["default"]);
    expect(el.querySelector('[data-testid="local-row"]')).toBeNull();
    const opts = Array.from(sel("向量模型云端档案").options).map((o) => o.textContent);
    expect(opts).toEqual(["默认云端配置", "硅基流动(未保存密钥)"]);
    expect(el.querySelector<HTMLInputElement>('input[aria-label="云端向量模型名"]')!.value).toBe("emb");
  });

  it("所选档案没有密钥时给出提示;默认配置填了未保存的密钥则不提示", () => {
    const form = { ...base, embedProvider: "openai_compat", embedProfile: "p1", embedCloudModel: "emb" } as Settings;
    render(form, "embed", ["default"]);
    expect(el.textContent).toContain("还没有 API Key");
    render({ ...form, embedProfile: "default" } as Settings, "embed", [], "sk-typed");
    expect(el.textContent).not.toContain("还没有 API Key");
  });

  it("测试:默认配置传入未保存的密钥;成功显示对号;模型名为空时按钮禁用", async () => {
    api.testCloudModel.mockResolvedValue("连接成功,向量模型 emb 可用(维度 512)");
    render({ ...base, embedProvider: "openai_compat", embedCloudModel: "emb" }, "embed", ["default"], "sk-typed");
    const btn = () => Array.from(el.querySelectorAll("button")).find((b) => b.textContent === "测试")!;
    await act(async () => { btn().click(); });
    await flush();
    expect(api.testCloudModel).toHaveBeenCalledWith("embed", null, "emb", "sk-typed");
    expect(el.querySelector('[data-testid="conn-result"]')?.textContent).toContain("连接成功");
    render({ ...base, embedProvider: "openai_compat", embedCloudModel: " " });
    expect(btn().disabled).toBe(true);
  });

  it("测试失败显示原因;非默认档案传档案标识且不带密钥", async () => {
    api.testCloudModel.mockRejectedValue(new Error("接口地址或模型名不存在"));
    render({ ...base, visionProvider: "openai_compat", visionProfile: "p1", visionCloudModel: "vl" }, "vision", ["p1"], "sk-typed");
    const btn = Array.from(el.querySelectorAll("button")).find((b) => b.textContent === "测试")!;
    await act(async () => { btn.click(); });
    await flush();
    expect(api.testCloudModel).toHaveBeenCalledWith("vision", "p1", "vl", null);
    expect(el.querySelector('[data-testid="conn-result"]')?.textContent).toContain("接口地址或模型名不存在");
  });
});
