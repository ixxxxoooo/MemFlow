// @author ygw
// 问答状态:会话、消息、流式输出(token 用 rAF 合并刷新,避免高频重渲染)

import { create } from "zustand";
import { api } from "./api";
import type { ChatEvent, Session, Source } from "./types";

export interface UiMessage {
  key: string;
  role: "user" | "assistant";
  content: string;
  sources: Source[];
  streaming: boolean;
  error?: string;
}

interface ChatState {
  sessionId: number | null;
  messages: UiMessage[];
  sessions: Session[];
  busy: boolean;
  loadSessions: () => Promise<void>;
  openSession: (id: number) => Promise<void>;
  newSession: () => void;
  removeSession: (id: number) => Promise<void>;
  send: (question: string) => Promise<void>;
  stop: () => Promise<void>;
}

let keySeq = 1;
const nextKey = () => `m${keySeq++}`;

export const useChat = create<ChatState>((set, get) => ({
  sessionId: null,
  messages: [],
  sessions: [],
  busy: false,

  loadSessions: async () => {
    try {
      set({ sessions: await api.listSessions() });
    } catch {
      /* 忽略:历史列表不是关键路径 */
    }
  },

  openSession: async (id) => {
    if (get().busy) return;
    const msgs = await api.getSessionMessages(id);
    set({
      sessionId: id,
      messages: msgs.map((m) => ({
        key: nextKey(), role: m.role, content: m.content, sources: m.sources ?? [], streaming: false,
      })),
    });
  },

  newSession: () => {
    if (get().busy) return;
    set({ sessionId: null, messages: [] });
  },

  removeSession: async (id) => {
    await api.deleteSession(id);
    if (get().sessionId === id) set({ sessionId: null, messages: [] });
    await get().loadSessions();
  },

  stop: async () => {
    await api.chatCancel().catch(() => undefined);
  },

  send: async (question) => {
    const q = question.trim();
    if (!q || get().busy) return;
    const assistantKey = nextKey();
    set((s) => ({
      busy: true,
      messages: [
        ...s.messages,
        { key: nextKey(), role: "user", content: q, sources: [], streaming: false },
        { key: assistantKey, role: "assistant", content: "", sources: [], streaming: true },
      ],
    }));
    const patch = (fn: (m: UiMessage) => UiMessage) =>
      set((s) => ({ messages: s.messages.map((m) => (m.key === assistantKey ? fn(m) : m)) }));

    // token 缓冲:每帧最多刷新一次
    let buffer = "";
    let raf = 0;
    const flush = () => {
      raf = 0;
      if (!buffer) return;
      const chunk = buffer;
      buffer = "";
      patch((m) => ({ ...m, content: m.content + chunk }));
    };
    const onEvent = (e: ChatEvent) => {
      switch (e.event) {
        case "session":
          set({ sessionId: e.data.sessionId });
          break;
        case "sources":
          patch((m) => ({ ...m, sources: e.data.sources }));
          break;
        case "token":
          buffer += e.data.text;
          if (!raf) raf = requestAnimationFrame(flush);
          break;
        case "done":
          if (raf) cancelAnimationFrame(raf);
          buffer = "";
          patch((m) => ({ ...m, content: e.data.answer, streaming: false }));
          break;
      }
    };
    try {
      await api.chatAsk(get().sessionId, q, onEvent);
    } catch (err) {
      if (raf) cancelAnimationFrame(raf);
      buffer = "";
      patch((m) => ({ ...m, streaming: false, error: (err as Error).message }));
    } finally {
      patch((m) => ({ ...m, streaming: false }));
      set({ busy: false });
      void get().loadSessions();
    }
  },
}));
