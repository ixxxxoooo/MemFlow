// @author ixxxxoooo
// 闪念胶囊:全局唤起的极简输入框。回车即保存并收起;支持粘贴截图、拖入文件。

import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { captureMessage, useCapture } from "../lib/useCapture";
import { AttachChips } from "../main/components/AttachChips";
import { SparkleIcon } from "../main/components/icons";

const STALE_DRAFT_MS = 5 * 60 * 1000;

export function Capsule() {
  const { text, setText, files, error, setError, busy, addPaths, removeFile, onPaste, reset, submit: submitCapture } = useCapture();
  const [dragging, setDragging] = useState(false);
  const ta = useRef<HTMLTextAreaElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const hiddenAt = useRef(Date.now());

  const focusInput = useCallback(() => {
    const el = ta.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, []);

  const hide = useCallback(async () => {
    hiddenAt.current = Date.now();
    await api.hideCapsule().catch(() => undefined);
  }, []);

  // 每次被唤起:聚焦;草稿超过 5 分钟则清空
  useEffect(() => {
    focusInput();
    const p = listen("capsule-shown", () => {
      if (Date.now() - hiddenAt.current > STALE_DRAFT_MS) reset();
      setError("");
      setTimeout(focusInput, 30);
    });
    return () => { p.then((f) => f()); };
  }, [focusInput, reset, setError]);

  // 窗口高度随内容自适应
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      void api.resizeCapsule(Math.ceil(el.getBoundingClientRect().height));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // 拖入文件
  useEffect(() => {
    const p = getCurrentWebview().onDragDropEvent((e) => {
      const t = e.payload.type;
      if (t === "enter" || t === "over") setDragging(true);
      else if (t === "leave") setDragging(false);
      else if (e.payload.type === "drop") {
        setDragging(false);
        addPaths(e.payload.paths);
        focusInput();
      }
    });
    return () => { p.then((f) => f()); };
  }, [focusInput, addPaths]);

  // 自动增高文本框
  useLayoutEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 150)}px`;
  }, [text]);

  const submit = async () => {
    const r = await submitCapture();
    if (!r) return;
    await hide();
    void api.notifyToast(captureMessage(r));
  };

  return (
    <div
      ref={root}
      className={`flex w-full flex-col rounded-[18px] border border-[var(--line)] ${dragging ? "flow-border" : ""}`}
      style={{ background: "var(--glass)" }}
    >
      <div className="flex items-start gap-3 px-4 py-[18px]" data-tauri-drag-region>
        <SparkleIcon size={20} className="mt-[2px] shrink-0 text-[var(--accent)]" />
        <textarea
          ref={ta}
          rows={1}
          value={text}
          autoFocus
          disabled={busy}
          placeholder={dragging ? "松手即可归档…" : "记下一个想法 · 粘贴截图 · 拖入文件"}
          className="max-h-[150px] min-h-[24px] flex-1 resize-none bg-transparent text-[16px] leading-6 outline-none placeholder:text-[var(--fg-faint)]"
          onChange={(e) => setText(e.target.value)}
          onPaste={onPaste}
          onKeyDown={(e) => {
            if (e.key === "Escape") { e.preventDefault(); void hide(); }
            else if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void submit(); }
          }}
        />
        <kbd className="mt-0.5 shrink-0 rounded border border-[var(--line)] px-1.5 text-[11px] text-[var(--fg-faint)]">↵</kbd>
      </div>
      <AttachChips files={files} onRemove={removeFile} className="px-4 pb-3.5" />
      {error && <div className="px-4 pb-3 text-xs text-[var(--err)]">{error}</div>}
      {(text || files.length > 0) && !error && (
        <div className="px-4 pb-2.5 text-[11px] text-[var(--fg-faint)]">Enter 保存 · Shift+Enter 换行 · Esc 收起(草稿会保留)</div>
      )}
    </div>
  );
}
