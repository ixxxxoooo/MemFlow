// @author ygw
// Markdown 渲染:支持 GFM、引用角标 [n] 点击溯源、```mermaid 代码块渲染为流程图

import { memo, useMemo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { linkCitations } from "../../lib/citations";
import type { Source } from "../../lib/types";
import { MermaidCard } from "./MermaidCard";

interface Props {
  text: string;
  sources?: Source[];
  streaming?: boolean;
  onCite?: (s: Source) => void;
}

/** 判断流式文本末尾是否有尚未闭合的代码围栏 */
function hasOpenFence(text: string): boolean {
  return (text.match(/```/g)?.length ?? 0) % 2 === 1;
}

export const Markdown = memo(function Markdown({ text, sources = [], streaming, onCite }: Props) {
  const content = useMemo(() => linkCitations(text, sources.length), [text, sources.length]);
  const openFence = !!streaming && hasOpenFence(text);

  const components = useMemo<Components>(
    () => ({
      a({ href, children }) {
        if (href?.startsWith("cite:")) {
          const n = Number(href.slice(5));
          const src = sources.find((s) => s.n === n);
          return (
            <button
              type="button"
              className="mx-0.5 inline-flex h-[18px] min-w-[18px] -translate-y-px items-center justify-center rounded-full bg-[var(--accent-soft)] px-1 align-middle text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-[var(--on-accent)]"
              onClick={() => src && onCite?.(src)}
              title={src?.title ?? `来源 ${n}`}
            >
              {n}
            </button>
          );
        }
        return <a href={href} target="_blank" rel="noreferrer">{children}</a>;
      },
      code({ className, children }) {
        const lang = /language-(\w+)/.exec(className ?? "")?.[1];
        const code = String(children).replace(/\n$/, "");
        if (lang === "mermaid") {
          return <MermaidCard code={code} live={openFence} />;
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
    [sources, onCite, openFence],
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
