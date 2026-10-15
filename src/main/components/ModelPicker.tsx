// @author ixxxxoooo
// 对话模型选择器:输入框旁的下拉,列出本地已安装与已配置密钥的云端模型

import { useEffect, useRef, useState } from "react";
import { defaultRef, effectiveRef, useChat } from "../../lib/chatStore";
import { useApp } from "../../lib/store";
import { BrandIcon } from "./BrandIcon";
import { CheckIcon, ChevronDownIcon } from "./icons";

export function ModelPicker() {
  const { models, modelRef, setModelRef, loadModels, busy } = useChat();
  const settings = useApp((s) => s.status?.settings);
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  // 设置变更(新增模型/密钥)后刷新列表
  const keyed = useApp((s) => s.status?.keyedProfiles.join(",") ?? "");
  const textModel = settings?.textModel;
  useEffect(() => { void loadModels(); }, [loadModels, keyed, textModel, settings?.llmProvider, settings?.llmModel]);

  // 点击外部关闭
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => { if (!box.current?.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);

  const ref = effectiveRef(modelRef, models, settings);
  const current = models.find((m) => m.ref === ref);
  // 列表尚未返回时,用设置里的默认模型名占位
  const fallbackName = settings
    ? (ref === defaultRef(settings) && settings.llmProvider === "ollama" ? settings.textModel : settings.llmModel)
    : "";
  const label = current?.label ?? fallbackName ?? "模型";

  const local = models.filter((m) => m.provider === "ollama");
  const cloud = models.filter((m) => m.provider !== "ollama");

  const item = (m: (typeof models)[number]) => (
    <button
      key={m.ref}
      className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-[var(--bg-soft)]"
      onClick={() => { setModelRef(m.ref); setOpen(false); }}
    >
      <BrandIcon model={m.model} preset={m.preset} provider={m.provider} size={16} />
      <span className="min-w-0 flex-1 truncate">{m.label}</span>
      {m.ref === ref && <CheckIcon size={14} className="text-[var(--accent)]" />}
    </button>
  );

  return (
    <div ref={box} className="relative">
      <button
        type="button"
        disabled={busy}
        title="切换回答所用的模型"
        className="flex h-9 max-w-[200px] items-center gap-1.5 rounded-xl px-2.5 text-[12.5px] text-[var(--fg-dim)] hover:bg-[var(--bg-soft)] disabled:opacity-50"
        onClick={() => setOpen((v) => !v)}
      >
        <BrandIcon model={current?.model ?? label} preset={current?.preset} provider={current?.provider ?? settings?.llmProvider} size={16} />
        <span className="truncate">{label}</span>
        <ChevronDownIcon size={13} />
      </button>
      {open && (
        <div className="pop-in absolute bottom-11 left-0 z-30 max-h-72 w-64 overflow-y-auto rounded-xl border border-[var(--line)] bg-[var(--bg-elev)] p-1.5 shadow-xl">
          {models.length === 0 && <div className="p-3 text-xs text-[var(--fg-dim)]">暂无可用模型,请先在设置里下载本地模型或配置云端 API Key。</div>}
          {local.length > 0 && <div className="px-2.5 pb-1 pt-1.5 text-[10.5px] font-semibold uppercase tracking-wide text-[var(--fg-faint)]">本地 Ollama</div>}
          {local.map(item)}
          {cloud.length > 0 && <div className="px-2.5 pb-1 pt-2 text-[10.5px] font-semibold uppercase tracking-wide text-[var(--fg-faint)]">云端 API</div>}
          {cloud.map(item)}
        </div>
      )}
    </div>
  );
}
