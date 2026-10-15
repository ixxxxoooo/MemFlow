// @author ygw
// Markdown 渲染:支持 GFM、引用角标 [n] 点击溯源、```mermaid 代码块渲染为流程图

import { memo, useMemo, useRef } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { linkCitations } from "../../lib/citations";
import { isFenceClosed } from "../../lib/markdownFence";
import type { Source } from "../../lib/types";
import { MermaidCard } from "./MermaidCard";

interface Props {
  text: string;
  sources?: Source[];
  streaming?: boolean;
  onCite?: (s: Source) => void;
}

export const Markdown = memo(function Markdown({ text, sources = [], streaming, onCite }: Props) {
  const content = useMemo(() => linkCitations(text, sources.length), [text, sources.length]);
  // 渲染器通过 ref 读取最新值,使 components 引用永远稳定;
  // 否则 sources/onCite 一变,react-markdown 会卸载重建所有自定义节点(流程图因此闪烁、重复渲染)
  const live = useRef({ content, sources, streaming: !!streaming, onCite });
  live.current = { content, sources, streaming: !!streaming, onCite };

  const components = useMemo<Components>(
    () => ({
      a({ href, children }) {
        if (href?.startsWith("cite:")) {
          const n = Number(href.slice(5));
          const src = live.current.sources.find((s) => s.n === n);
          return (
            <button
              type="button"
              className="mx-0.5 inline-flex h-[18px] min-w-[18px] -translate-y-px items-center justify-center rounded-full bg-[var(--accent-soft)] px-1 align-middle text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-[var(--on-accent)]"
              onClick={() => src && live.current.onCite?.(src)}
              title={src?.title ?? `来源 ${n}`}
            >
              {n}
            </button>
          );
        }
        return <a href={href} target="_blank" rel="noreferrer">{children}</a>;
      },
      code({ className, children, node }) {
        const lang = /language-(\w+)/.exec(className ?? "")?.[1];
        const code = String(children).replace(/\n$/, "");
        if (lang === "mermaid") {
          const { content: md, streaming: isStreaming } = live.current;
          const a = node?.position?.start.offset;
          const b = node?.position?.end.offset;
          // 仅看当前块自身是否写完,而不是整条消息的围栏奇偶
          const open = isStreaming && (a == null || b == null ? true : !isFenceClosed(md.slice(a, b)));
          return <MermaidCard code={code} live={open} deferRepair={isStreaming} />;
        }
        if (!className) return <code>{children}</code>;
        return <code className={className}>{children}</code>;
      },
      pre({ children }) {
        // mermaid 卡片自带容器,避免外层再套 <pre>
        const child = Array.isArray(children) ? children[0] : children;
        const cls = (child as { props?: { className?: string } } | undefined)?.props?.className ?? "";
        if (/language-mermaid/.test(cls)) return <>{children}</>;
        return <pre>{children}</pre>;
      },
    }),
    [],
  );

  return (
    <div className="md selectable">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={components}
        urlTransform={(url) => (url.startsWith("cite:") ? url : /^(https?:|mailto:)/i.test(url) ? url : "")}
      >
        {content}
      </ReactMarkdown>
      {streaming && <span className="typing-dot" />}
    </div>
  );
});
