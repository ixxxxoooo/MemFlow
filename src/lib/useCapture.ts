// @author ygw
// 记录输入的共用逻辑:文字、附件(拖入/选择/粘贴)、提交到默认知识库。
// 闪念胶囊与主界面的「记一笔」对话框共用,避免两套实现。

import { useCallback, useRef, useState, type ClipboardEvent } from "react";
import { api } from "./api";
import { DEFAULT_KB_ID } from "./kbScope";
import type { CaptureResult } from "./types";

export interface Attach {
  id: number;
  path: string;
  name: string;
  /** 粘贴图片的本地预览(object URL) */
  preview?: string;
}

let seq = 1;

/** 提交结果的提示文案 */
export function captureMessage(r: CaptureResult): string {
  const msg = r.created.length > 0 ? "✨ 已交由 AI 归档" : r.duplicates.length > 0 ? "已存在,已跳过重复内容" : "已保存";
  return r.errors.length ? `${msg}(${r.errors.length} 项失败)` : msg;
}

export function useCapture() {
  const [text, setText] = useState("");
  const [files, setFiles] = useState<Attach[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  // 提交时读取最新值,避免键盘事件里拿到过期闭包
  const ref = useRef({ text, files, busy });
  ref.current = { text, files, busy };

  /** 追加本地文件路径(自动去重) */
  const addPaths = useCallback((paths: string[]) => {
    setFiles((prev) => [
      ...prev,
      ...paths.filter((p) => !prev.some((x) => x.path === p)).map((path) => ({ id: seq++, path, name: path.split("/").pop() ?? path })),
    ]);
  }, []);

  const removeFile = useCallback((id: number) => {
    setFiles((prev) => {
      const f = prev.find((x) => x.id === id);
      if (f?.preview) URL.revokeObjectURL(f.preview);
      return prev.filter((x) => x.id !== id);
    });
  }, []);

  /** 粘贴截图/文件:先落到临时目录,再作为附件 */
  const onPaste = useCallback(async (e: ClipboardEvent) => {
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
  }, []);

  /** 清空草稿 */
  const reset = useCallback(() => {
    ref.current.files.forEach((f) => f.preview && URL.revokeObjectURL(f.preview));
    setText("");
    setFiles([]);
    setError("");
  }, []);

  /**
   * 提交到默认知识库
   *
   * 返回: 成功返回后端结果(已清空草稿);无内容、正在提交或失败返回 null(失败原因在 error)
   */
  const submit = useCallback(async (): Promise<CaptureResult | null> => {
    const { text: t, files: fs, busy: b } = ref.current;
    const trimmed = t.trim();
    if ((!trimmed && fs.length === 0) || b) return null;
    setBusy(true);
    setError("");
    try {
      const r = await api.captureSubmit(trimmed || null, fs.map((f) => f.path), DEFAULT_KB_ID);
      reset();
      return r;
    } catch (e) {
      setError((e as Error).message);
      return null;
    } finally {
      setBusy(false);
    }
  }, [reset]);

  return { text, setText, files, setFiles, error, setError, busy, addPaths, removeFile, onPaste, reset, submit };
}
