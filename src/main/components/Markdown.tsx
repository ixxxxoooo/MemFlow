// @author ixxxxoooo
// Markdown 渲染:支持 GFM、引用角标 [n] 点击溯源、```mermaid 代码块渲染为流程图

import { memo, useMemo, useRef } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { linkCitations } from "../../lib/citations";
import { lookupImage, type DocImage } from "../../lib/docImages";
import { isFenceClosed } from "../../lib/markdownFence";
import type { Source } from "../../lib/types";
import { MermaidCard } from "./MermaidCard";

interface Props {
  text: string;
  sources?: Source[];
  streaming?: boolean;
  onCite?: (s: Source) => void;
  /** 文档预览:正文里的图片写法 → 关联的图片记录 */
  images?: Map<string, DocImage>;
  /** 点击文档里的图片 */
  onImageClick?: (id: number) => void;
}

export const Markdown = memo(function Markdown({ text, sources = [], streaming, onCite, images, onImageClick }: Props) {
  const content = useMemo(() => linkCitations(text, sources.length), [text, sources.length]);
  // 渲染器通过 ref 读取最新值,使 components 引用永远稳定;
  // 否则 sources/onCite 一变,react-markdown 会卸载重建所有自定义节点(流程图因此闪烁、重复渲染)
  const live = useRef({ content, sources, streaming: !!streaming, onCite, images, onImageClick });
  live.current = { content, sources, streaming: !!streaming, onCite, images, onImageClick };

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
      img({ src, alt }) {
        const hit = lookupImage(live.current.images, src);
        if (!hit) {
          // 找不到关联图片(远程图、已缺失):只保留替代文字,不发起网络请求
          return alt ? <span className="text-[var(--fg-faint)]">[图片:{alt}]</span> : null;
        }
        return (
          <img
            src={hit.url}
            alt={alt ?? hit.name ?? ""}
            loading="lazy"
            className="my-2 max-h-[420px] max-w-full cursor-zoom-in rounded-lg border border-[var(--line)] object-contain"
            onClick={() => live.current.onImageClick?.(hit.id)}
          />
        );
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
        urlTransform={(url, key) => {
          if (url.startsWith("cite:")) return url;
          // 图片地址原样交给渲染器,由它对应到关联的图片记录(不会被当作网络地址加载)
          if (key === "src") return /^(https?:|data:|javascript:)/i.test(url) ? "" : url;
          return /^(https?:|mailto:)/i.test(url) ? url : "";
        }}
      >
        {content}
      </ReactMarkdown>
      {streaming && <span className="typing-dot" />}
    </div>
  );
});
