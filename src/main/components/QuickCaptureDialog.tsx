// @author ygw
// 「记一笔」对话框:在主界面任何页面随手记一句话 / 粘贴截图 / 添加文件,存入默认知识库。
// 与闪念胶囊共用 useCapture;窗口内拖入的文件由 App 转发为 `memflow-capture-files` 事件追加到这里。

import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { ALL_EXTENSIONS, FORMATS_SUMMARY } from "../../lib/formats";
import { DEFAULT_KB_ID } from "../../lib/kbScope";
import { useKb } from "../../lib/kbStore";
import { useApp } from "../../lib/store";
import { captureMessage, useCapture } from "../../lib/useCapture";
import { AttachChips } from "./AttachChips";
import { SparkleIcon, UploadIcon } from "./icons";

/** 主窗口把拖入的路径转发给对话框所用的事件名 */
export const CAPTURE_FILES_EVENT = "memflow-capture-files";

export function QuickCaptureDialog() {
  const { captureOpen, closeCapture, toast, bumpData, refreshStatus } = useApp();
  if (!captureOpen) return null;
  return <Inner onClose={closeCapture} toast={toast} onSaved={() => { bumpData(); void refreshStatus(); }} />;
}

function Inner({ onClose, toast, onSaved }: { onClose: () => void; toast: (t: string, k?: "info" | "ok" | "err") => void; onSaved: () => void }) {
  const { text, setText, files, error, busy, addPaths, removeFile, onPaste, submit } = useCapture();
  const kbName = useKb((s) => s.kbs.find((k) => k.id === DEFAULT_KB_ID)?.name) ?? "默认知识库";
  const ta = useRef<HTMLTextAreaElement>(null);

  // 窗口内拖入的文件
  useEffect(() => {
    const h = (e: Event) => addPaths((e as CustomEvent<string[]>).detail);
    window.addEventListener(CAPTURE_FILES_EVENT, h);
    return () => window.removeEventListener(CAPTURE_FILES_EVENT, h);
  }, [addPaths]);

  // 自动增高文本框
  useLayoutEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  }, [text]);

  const save = async () => {
    const r = await submit();
    if (!r) return;
    toast(captureMessage(r), r.errors.length ? "err" : "ok");
    onSaved();
    onClose();
  };

  const pick = async () => {
    const picked = await open({ multiple: true, filters: [{ name: "文档与图片", extensions: ALL_EXTENSIONS }] });
    if (picked) addPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const canSave = (text.trim() || files.length > 0) && !busy;

  return createPortal(
    <div className="fixed inset-0 z-[70] flex items-start justify-center bg-black/45 p-6 pt-[14vh]" onMouseDown={onClose}>
      <div role="dialog" aria-label="记一笔" className="pop-in w-[560px] rounded-2xl border border-[var(--line)] bg-[var(--bg)] p-4 shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <div className="flex items-start gap-3">
          <SparkleIcon size={18} className="mt-1.5 shrink-0 text-[var(--accent)]" />
          <textarea
            ref={ta}
            autoFocus
            rows={3}
            value={text}
            disabled={busy}
            placeholder="记下一个想法 · 粘贴截图 · 添加文件"
            className="max-h-[220px] min-h-[72px] flex-1 resize-none bg-transparent text-[15px] leading-6 outline-none placeholder:text-[var(--fg-faint)]"
            onChange={(e) => setText(e.target.value)}
            onPaste={(e) => void onPaste(e)}
            onKeyDown={(e) => {
              if (e.key === "Escape") { e.preventDefault(); onClose(); }
              else if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void save(); }
            }}
          />
        </div>
        <AttachChips files={files} onRemove={removeFile} className="mt-2 pl-8" />
        {error && <div className="mt-2 pl-8 text-xs text-[var(--err)]">{error}</div>}
        <div className="mt-3 flex items-center gap-2 pl-8">
          <button className="btn btn-ghost btn-sm" title={`支持 ${FORMATS_SUMMARY}`} onClick={() => void pick()} disabled={busy}><UploadIcon size={13} />添加文件</button>
          <span className="min-w-0 flex-1 text-[11px] leading-snug text-[var(--fg-faint)]">存入「{kbName}」· Enter 保存 · Shift+Enter 换行 · Esc 关闭</span>
          <button className="btn btn-secondary" onClick={onClose} disabled={busy}>取消</button>
          <button className="btn btn-primary" onClick={() => void save()} disabled={!canSave}>{busy ? "保存中…" : "保存"}</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
