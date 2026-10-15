// @author ixxxxoooo
import { describe, expect, it } from "vitest";
import { embedIdentity, needsOllama } from "./modelSource";

describe("embedIdentity", () => {
  const base = { embedModel: "bge-m3", embedProfile: "default", embedCloudModel: " text-embedding-v3 " };
  it("本地沿用模型名,云端带档案与去空格的模型名", () => {
    expect(embedIdentity({ ...base, embedProvider: "ollama" })).toBe("bge-m3");
    expect(embedIdentity({ ...base, embedProvider: "openai_compat" })).toBe("cloud:default:text-embedding-v3");
  });
});

describe("needsOllama", () => {
  it("只要有必需的本地模型就需要 Ollama", () => {
    expect(needsOllama([{ local: false, required: true }, { local: true, required: false }])).toBe(false);
    expect(needsOllama([{ local: false, required: true }, { local: true, required: true }])).toBe(true);
    expect(needsOllama([])).toBe(false);
  });
});
