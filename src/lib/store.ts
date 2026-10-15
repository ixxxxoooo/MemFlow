// @author ixxxxoooo
// 全局应用状态(视图、状态快照、详情抽屉、轻提示)

import { create } from "zustand";
import { api } from "./api";
import { isFakeIpError } from "./llmPresets";
import type { AppStatus, PullProgress, QueueStatus } from "./types";

export type View = "chat" | "timeline" | "docs" | "settings";

export interface ToastItem {
  id: number;
  text: string;
  kind: "info" | "ok" | "err";
}

interface AppState {
  view: View;
  status: AppStatus | null;
  queue: QueueStatus | null;
  drawer: { itemId: number; chunkId?: number } | null;
  /** 文档弹窗(预览/编辑);chunkId 用于定位被引用的切片 */
  docModal: { itemId: number; chunkId?: number } | null;
  toasts: ToastItem[];
  /** 当前正在下载的模型(全局唯一,与后端"同时仅一个下载"一致) */
  pull: { name: string; direct: boolean; progress: PullProgress | null } | null;
  /** 普通下载被 fake-ip 拦截后,提示用户改用直连下载的模型名 */
  pullHint: string | null;
  /** 数据变化计数,列表视图据此刷新 */
  dataVersion: number;
  /** 「记一笔」对话框是否打开 */
  captureOpen: boolean;
  openCapture: () => void;
  closeCapture: () => void;
  /** 档案库里当前打开的知识库;null 表示在知识库列表页 */
  docsKb: number | null;
  setDocsKb: (id: number | null) => void;
  setView: (v: View) => void;
  setQueue: (q: QueueStatus) => void;
  refreshStatus: () => Promise<void>;
  bumpData: () => void;
  openDrawer: (itemId: number, chunkId?: number) => void;
  closeDrawer: () => void;
  openDocModal: (itemId: number, chunkId?: number) => void;
  closeDocModal: () => void;
  /** 条目被删除:关闭正在展示它们的抽屉/弹窗并刷新数据 */
  itemsDeleted: (ids: number[]) => void;
  toast: (text: string, kind?: ToastItem["kind"]) => void;
  startPull: (name: string, direct: boolean) => Promise<void>;
  /** 下载本地重排序模型(与 Ollama 模型共用"同时仅一个下载"名额,name 为 rerank:<id>) */
  startRerankPull: (id: string) => Promise<void>;
}

let toastSeq = 1;

export const useApp = create<AppState>((set, get) => ({
  view: "chat",
  status: null,
  queue: null,
  drawer: null,
  docModal: null,
  toasts: [],
  pull: null,
  pullHint: null,
  dataVersion: 0,
  captureOpen: false,
  openCapture: () => set({ captureOpen: true }),
  closeCapture: () => set({ captureOpen: false }),
  docsKb: null,
  setDocsKb: (docsKb) => set({ docsKb }),
  setView: (view) => set({ view }),
  setQueue: (queue) => set({ queue }),
  refreshStatus: async () => {
    try {
      const status = await api.appStatus();
      set({ status, queue: status.queue });
    } catch (e) {
      get().toast(`读取状态失败:${(e as Error).message}`, "err");
    }
  },
  bumpData: () => set((s) => ({ dataVersion: s.dataVersion + 1 })),
  openDrawer: (itemId, chunkId) => set({ drawer: { itemId, chunkId } }),
  closeDrawer: () => set({ drawer: null }),
  openDocModal: (itemId, chunkId) => set({ docModal: { itemId, chunkId } }),
  closeDocModal: () => set({ docModal: null }),
  itemsDeleted: (ids) =>
    set((s) => ({
      // 被删除的条目若正在抽屉/弹窗中展示,先关闭再刷新,避免重新读取已不存在的记录而弹错
      drawer: s.drawer && ids.includes(s.drawer.itemId) ? null : s.drawer,
      docModal: s.docModal && ids.includes(s.docModal.itemId) ? null : s.docModal,
      dataVersion: s.dataVersion + 1,
    })),
  toast: (text, kind = "info") => {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts, { id, text, kind }] }));
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), kind === "err" ? 6000 : 2800);
  },
  startPull: async (name, direct) => {
    if (get().pull) return;
    set({ pull: { name, direct, progress: null }, pullHint: null });
    const onProgress = (progress: PullProgress) =>
      set((s) => (s.pull ? { pull: { ...s.pull, progress } } : {}));
    try {
      await (direct ? api.pullModelDirect(name, onProgress) : api.pullModel(name, onProgress));
      get().toast(`${name} 下载完成`, "ok");
    } catch (e) {
      const msg = (e as Error).message;
      if (!direct && isFakeIpError(msg)) {
        set({ pullHint: name });
        get().toast("下载被代理的 fake-ip 拦截,请点击「直连下载」重试", "err");
      } else {
        get().toast(`下载失败:${msg}`, "err");
      }
    } finally {
      set({ pull: null });
      void get().refreshStatus();
    }
  },
  startRerankPull: async (id) => {
    if (get().pull) return;
    set({ pull: { name: `rerank:${id}`, direct: false, progress: null } });
    const onProgress = (progress: PullProgress) =>
      set((s) => (s.pull ? { pull: { ...s.pull, progress } } : {}));
    try {
      await api.downloadRerankModel(id, onProgress);
      get().toast("重排序模型下载完成", "ok");
    } catch (e) {
      get().toast(`下载失败:${(e as Error).message}`, "err");
    } finally {
      set({ pull: null });
    }
  },
}));
