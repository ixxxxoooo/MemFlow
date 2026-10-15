// @author ixxxxoooo
// Markdown 回归:sources/onCite/streaming 变化时,流程图节点不得被卸载重建(曾导致闪烁与一直渲染中)

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Source } from "../../lib/types";

const events: string[] = [];
const props: { live?: boolean; deferRepair?: boolean }[] = [];

vi.mock("./MermaidCard", async () => {
  const React = await import("react");
  return {
    MermaidCard: (p: { code: string; live?: boolean; deferRepair?: boolean }) => {
      props.push({ live: p.live, deferRepair: p.deferRepair });
      React.useEffect(() => {
        events.push("mount");
        return () => { events.push("unmount"); };
      }, []);
      return React.createElement("div", { "data-testid": "mermaid" }, p.code);
    },
  };
});

import { Markdown } from "./Markdown";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const src = (n: number): Source => ({ n, itemId: n, chunkId: n, title: `T${n}`, kind: "note", snippet: "" } as unknown as Source);
const FLOW = "前文\n\n```mermaid\nflowchart TD\nA-->B\n```\n\n后文";

describe("Markdown 流程图节点稳定性", () => {
  let el: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    events.length = 0;
    props.length = 0;
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
  });
  afterEach(() => {
    act(() => root.unmount());
    el.remove();
  });

  it("来源、回调、流式状态变化不会让流程图卸载重建", () => {
    act(() => root.render(<Markdown text={FLOW} sources={[]} streaming onCite={() => undefined} />));
    act(() => root.render(<Markdown text={FLOW} sources={[src(1)]} streaming onCite={() => undefined} />));
    act(() => root.render(<Markdown text={FLOW} sources={[src(1), src(2)]} streaming={false} onCite={() => undefined} />));
    expect(events).toEqual(["mount"]);
  });

  it("流式中追加文字(块前内容不变)不会重建流程图", () => {
    act(() => root.render(<Markdown text={FLOW} streaming />));
    act(() => root.render(<Markdown text={FLOW + "继续输出一段话"} streaming />));
    act(() => root.render(<Markdown text={FLOW + "继续输出一段话,再多一点"} streaming />));
    expect(events).toEqual(["mount"]);
  });

  it("按块判断:已闭合的块在流式中不再算 live,尚未写完的块是 live", () => {
    act(() => root.render(<Markdown text={FLOW} streaming />));
    expect(props.at(-1)).toEqual({ live: false, deferRepair: true });
    props.length = 0;
    act(() => root.render(<Markdown text={"```mermaid\nflowchart TD\nA-->"} streaming />));
    expect(props.at(-1)).toEqual({ live: true, deferRepair: true });
    props.length = 0;
    act(() => root.render(<Markdown text={FLOW} />));
    expect(props.at(-1)).toEqual({ live: false, deferRepair: false });
  });

  it("后面再出现普通代码块,前面已闭合的流程图仍非 live(不再按整条消息围栏奇偶判断)", () => {
    act(() => root.render(<Markdown text={FLOW + "\n\n```js\nconst a ="} streaming />));
    const flow = props.find((p) => p.deferRepair);
    expect(flow?.live).toBe(false);
  });
});

describe("Markdown 图片渲染", () => {
  let el: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
  });
  afterEach(() => {
    act(() => root.unmount());
    el.remove();
  });

  it("关联到的图片内联显示,点击回调图片 id;未关联与远程图只显示替代文字且不发请求", () => {
    const images = new Map([["../img/预算.png", { url: "asset://x/a.png", id: 7, name: "a.png" }]]);
    const clicked: number[] = [];
    act(() =>
      root.render(
        <Markdown
          text={"![预算](../img/%E9%A2%84%E7%AE%97.png)\n\n![缺失](none.png)\n\n![远程](https://e.com/x.png)"}
          images={images}
          onImageClick={(id) => clicked.push(id)}
        />,
      ),
    );
    const imgs = el.querySelectorAll("img");
    expect(imgs).toHaveLength(1);
    expect(imgs[0].getAttribute("src")).toBe("asset://x/a.png");
    act(() => imgs[0].click());
    expect(clicked).toEqual([7]);
    expect(el.textContent).toContain("[图片:缺失]");
    expect(el.textContent).toContain("[图片:远程]");
  });
});

