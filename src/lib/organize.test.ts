// @author ygw
import { describe, expect, it } from "vitest";
import type { ChatModelOption, NoteBrief, Settings } from "./types";
import { briefName, mergeCountOk, organizeModel, pct, refIsCloud, useOrganize } from "./organize";

const models = [
  { ref: "ollama:qwen2.5:7b", label: "qwen2.5:7b", provider: "ollama", model: "qwen2.5:7b" },
  { ref: "cloud:p1", label: "DeepSeek", provider: "cloud", model: "deepseek-chat" },
] as unknown as ChatModelOption[];

describe("AI 整理辅助", () => {
  it("云端判断", () => {
    expect(refIsCloud("ollama:qwen2.5:7b")).toBe(false);
    expect(refIsCloud("cloud:p1")).toBe(true);
    expect(refIsCloud(null)).toBe(false);
  });

  it("整理模型跟随选择,未选时回退设置", () => {
    expect(organizeModel("cloud:p1", models, undefined)).toEqual({ ref: "cloud:p1", label: "DeepSeek", cloud: true });
    const local = { llmProvider: "ollama", textModel: "qwen2.5:7b", llmModel: "x" } as unknown as Settings;
    expect(organizeModel(null, models, local)).toEqual({ ref: null, label: "qwen2.5:7b", cloud: false });
    const cloud = { llmProvider: "openai", textModel: "q", llmModel: "gpt" } as unknown as Settings;
    expect(organizeModel("不存在", models, cloud)).toEqual({ ref: null, label: "gpt", cloud: true });
  });

  it("归并条数 2–12", () => {
    expect([1, 2, 12, 13].map(mergeCountOk)).toEqual([false, true, true, false]);
  });

  it("来源展示名与相似度", () => {
    const b = { id: 1, kbId: 1, title: null, preview: "老王说 A\n项目预算会缩减,需要重新排期并通知所有人", createdAt: 0 } as NoteBrief;
    expect(briefName(b, 10)).toBe("老王说 A 项目预算…");
    expect(briefName({ ...b, title: " 预算 " })).toBe("预算");
    expect(pct(0.826)).toBe("83%");
  });

  it("对话框状态切换", () => {
    const s = useOrganize.getState();
    s.openMerge([1, 2]);
    expect(useOrganize.getState().dialog).toEqual({ kind: "merge", ids: [1, 2] });
    s.openSingle(5);
    expect(useOrganize.getState().dialog).toEqual({ kind: "single", itemId: 5 });
    s.close();
    expect(useOrganize.getState().dialog).toBeNull();
  });
});
