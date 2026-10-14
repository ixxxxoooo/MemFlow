// @author ygw
// 全局应用状态(视图、状态快照、详情抽屉、轻提示)

import { create } from "zustand";
import { api } from "./api";
import type { AppStatus, QueueStatus } from "./types";

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
  toasts: ToastItem[];
  /** 数据变化计数,列表视图据此刷新 */
  dataVersion: number;
  setView: (v: View) => void;
  setQueue: (q: QueueStatus) => void;
  refreshStatus: () => Promise<void>;
  bumpData: () => void;
  openDrawer: (itemId: number, chunkId?: number) => void;
  closeDrawer: () => void;
  toast: (text: string, kind?: ToastItem["kind"]) => void;
}

let toastSeq = 1;

export const useApp = create<AppState>((set, get) => ({
  view: "chat",
  status: null,
  queue: null,
  drawer: null,
  toasts: [],
  dataVersion: 0,
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
  toast: (text, kind = "info") => {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts, { id, text, kind }] }));
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), kind === "err" ? 6000 : 2800);
  },
}));
