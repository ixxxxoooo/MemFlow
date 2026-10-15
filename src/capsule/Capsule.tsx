// @author ygw
// 闪念胶囊:全局唤起的极简输入框。回车即保存并收起;支持粘贴截图、拖入文件。

import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { DEFAULT_KB_ID } from "../lib/kbScope";
import { CloseIcon, FileIcon, ImageIcon, SparkleIcon } from "../main/components/icons";

interface Attach {
  id: number;
  path: string;
  name: string;
  /** 粘贴图片的本地预览(object URL) */
  preview?: string;
}

const STALE_DRAFT_MS = 5 * 60 * 1000;

let seq = 1;

export function Capsule() {
  const [text, setText] = useState("");
  const [files, setFiles] = useState<Attach[]>([]);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const ta = useRef<HTMLTextAreaElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const hiddenAt = useRef(Date.now());
  const stateRef = useRef({ text, files });
  stateRef.current = { text, files };

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
      if (Date.now() - hiddenAt.current > STALE_DRAFT_MS) {
        setText("");
        setFiles([]);
      }
      setError("");
      setTimeout(focusInput, 30);
    });
    return () => { p.then((f) => f()); };
  }, [focusInput]);

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
        const dropped: string[] = e.payload.paths;
        setDragging(false);
        setFiles((prev) => [
          ...prev,
          ...dropped.filter((p) => !prev.some((x) => x.path === p)).map((path) => ({ id: seq++, path, name: path.split("/").pop() ?? path })),
        ]);
        focusInput();
      }
    });
    return () => { p.then((f) => f()); };
  }, [focusInput]);

  // 自动增高文本框
  useLayoutEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 150)}px`;
  }, [text]);

  const onPaste = async (e: React.ClipboardEvent) => {
    const list = Array.from(e.clipboardData.files);
    if (list.length === 0) return;
    e.preventDefault();
    for (const f of list) {
      try {
        const path = await api.stageFile(f);
        const isImg = f.type.startsWith("image/");
        setFiles((prev) => [...prev, { id: seq++, path, name: f.name || "粘贴的图片", preview: isImg ? URL.createObjectURL(f) : undefined }]);
      } catch (err) {
        setError((err as Error).message);
      }
    }
  };

  const submit = async () => {
    const { text: t, files: fs } = stateRef.current;
    const trimmed = t.trim();
    if ((!trimmed && fs.length === 0) || busy) return;
    setBusy(true);
    setError("");
    try {
      // 胶囊不选知识库:一律存入默认知识库
      const r = await api.captureSubmit(trimmed || null, fs.map((f) => f.path), DEFAULT_KB_ID);
      fs.forEach((f) => f.preview && URL.revokeObjectURL(f.preview));
      setText("");
      setFiles([]);
      await hide();
      const msg = r.created.length > 0 ? "✨ 已交由 AI 归档" : r.duplicates.length > 0 ? "已存在,已跳过重复内容" : "已保存";
      void api.notifyToast(r.errors.length ? `${msg}(${r.errors.length} 项失败)` : msg);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
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
      {files.length > 0 && (
        <div className="flex flex-wrap gap-2 px-4 pb-3.5">
          {files.map((f) => (
            <div key={f.id} className="group relative flex items-center gap-2 overflow-hidden rounded-xl border border-[var(--line)] bg-[var(--bg-soft)]/70 p-1.5 pr-3 text-xs">
              {f.preview ? (
                <img src={f.preview} alt="" className="h-12 w-12 rounded-lg object-cover" />
              ) : /\.(png|jpe?g|webp|gif|bmp)$/i.test(f.name) ? (
                <ImageIcon size={18} className="m-3 text-[var(--accent)]" />
              ) : (
                <FileIcon size={18} className="m-3 text-[var(--accent)]" />
              )}
              <span className="max-w-[170px] truncate">{f.name}</span>
              <button
                className="absolute right-1 top-1 hidden rounded-full bg-black/55 p-0.5 text-white group-hover:block"
                onClick={() => { f.preview && URL.revokeObjectURL(f.preview); setFiles((p) => p.filter((x) => x.id !== f.id)); }}
              >
                <CloseIcon size={10} />
              </button>
            </div>
          ))}
        </div>
      )}
      {error && <div className="px-4 pb-3 text-xs text-[var(--err)]">{error}</div>}
      {(text || files.length > 0) && !error && (
        <div className="px-4 pb-2.5 text-[11px] text-[var(--fg-faint)]">Enter 保存 · Shift+Enter 换行 · Esc 收起(草稿会保留)</div>
      )}
    </div>
  );
}
