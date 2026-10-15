// @author ixxxxoooo
// 连接测试结果:成功显示绿色对号,失败显示原因

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { ConnResult } from "./ConnResult";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function render(state: Parameters<typeof ConnResult>[0]["state"]) {
  const el = document.createElement("div");
  const root = createRoot(el);
  act(() => root.render(<ConnResult state={state} />));
  return el;
}

describe("ConnResult", () => {
  it("无结果时不渲染", () => {
    expect(render(null).innerHTML).toBe("");
  });
  it("成功时显示绿色对号与「连接成功」", () => {
    const el = render({ ok: true, msg: "连接成功(deepseek,230ms)" });
    const r = el.querySelector('[data-testid="conn-result"]')!;
    expect(r.className).toContain("var(--ok)");
    expect(r.querySelector("svg")).toBeTruthy();
    expect(r.textContent).toContain("连接成功");
  });
  it("失败时显示红色与原因", () => {
    const el = render({ ok: false, msg: "401 鉴权失败" });
    const r = el.querySelector('[data-testid="conn-result"]')!;
    expect(r.className).toContain("var(--err)");
    expect(r.textContent).toContain("401 鉴权失败");
  });
});
