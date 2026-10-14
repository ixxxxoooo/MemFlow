// @author ygw
// Markdown 所见即所得编辑器(基于 Milkdown Crepe):标题/列表/表格/代码块/任务列表,输入即渲染

import { Crepe } from "@milkdown/crepe";
import "@milkdown/crepe/theme/common/style.css";
import { useEffect, useRef } from "react";

interface Props {
  /** 初始 Markdown 文本(仅挂载时读取,之后由编辑器自身维护) */
  value: string;
  /** 内容变化回调,参数为最新 Markdown */
  onChange: (markdown: string) => void;
}

export default function MarkdownEditor({ value, onChange }: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    // 关闭体积大或依赖后端能力的特性:图片上传、公式、AI、顶部栏、代码高亮
    const crepe = new Crepe({
      root,
      defaultValue: value,
      features: {
        [Crepe.Feature.ImageBlock]: false,
        [Crepe.Feature.Latex]: false,
        [Crepe.Feature.AI]: false,
        [Crepe.Feature.TopBar]: false,
        [Crepe.Feature.CodeMirror]: false,
      },
    });
    crepe.on((api) => {
      api.markdownUpdated((_ctx, markdown) => onChangeRef.current(markdown));
    });
    const ready = crepe.create();
    return () => {
      // 等创建完成后再销毁,避免 StrictMode 下重复挂载导致残留
      void ready.then(() => crepe.destroy());
    };
    // 只在挂载时初始化一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return <div ref={rootRef} className="mf-editor selectable" />;
}
