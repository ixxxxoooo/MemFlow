// @author ygw
import { describe, expect, it } from "vitest";
import { brandOf } from "./brands";

describe("brandOf", () => {
  it("按模型名识别主流品牌", () => {
    expect(brandOf("qwen2.5:7b")).toBe("qwen");
    expect(brandOf("deepseek-chat")).toBe("deepseek");
    expect(brandOf("llama3.1:8b")).toBe("meta");
    expect(brandOf("gemma2:9b")).toBe("gemma");
    expect(brandOf("glm-4-flash")).toBe("zhipu");
    expect(brandOf("moonshot-v1-8k")).toBe("kimi");
    expect(brandOf("gpt-4o-mini")).toBe("openai");
    expect(brandOf("Qwen/Qwen2.5-7B-Instruct")).toBe("qwen");
    expect(brandOf("yi:34b")).toBe("yi");
    expect(brandOf("mistral:7b")).toBe("mistral");
  });

  it("模型名无法识别时回退到预设,再回退到来源", () => {
    expect(brandOf("my-model", "siliconflow", "openai_compat")).toBe("siliconcloud");
    expect(brandOf("my-model", "custom", "ollama")).toBe("ollama");
    expect(brandOf("my-model", "custom", "openai_compat")).toBeNull();
  });

  it("不会把含 yi 字样的无关名称误判", () => {
    expect(brandOf("yield-model")).toBeNull();
  });
});
