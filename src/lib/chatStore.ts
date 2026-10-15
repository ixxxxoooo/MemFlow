// @author ygw
// 问答状态:会话、消息、流式输出(token 用 rAF 合并刷新,避免高频重渲染)

import { create } from "zustand";
import { api } from "./api";
import type { AnswerMeta, ChatEvent, ChatModelOption, Session, Settings, Source } from "./types";

export interface UiMessage {
  key: string;
  role: "user" | "assistant";
  content: string;
  sources: Source[];
  streaming: boolean;
  error?: string;
  /** 回答所用模型与耗时(仅助手消息,生成完成后才有) */
  meta?: AnswerMeta;
  /** 发送这一问时的检索/生成阶段提示 */
  stage?: string;
}

/** 与后端 default_ref 保持一致:未显式选择时使用的模型引用 */
export function defaultRef(s: Settings): string {
  return s.llmProvider === "ollama" ? `ollama:${s.textModel}` : "cloud:default";
}

/** 解析当前应使用的模型引用:显式选择且仍可用则用之,否则回退默认 */
export function effectiveRef(sel: string | null, models: ChatModelOption[], settings: Settings | undefined): string | null {
  if (sel && models.some((m) => m.ref === sel)) return sel;
  return settings ? defaultRef(settings) : null;
}

interface ChatState {
  sessionId: number | null;
  messages: UiMessage[];
  sessions: Session[];
  busy: boolean;
  /** 用户在输入框旁选择的模型引用;null 表示跟随设置里的默认模型 */
  modelRef: string | null;
  models: ChatModelOption[];
  loadModels: () => Promise<void>;
  setModelRef: (ref: string | null) => void;
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
  modelRef: null,
  models: [],

  loadModels: async () => {
    try {
      set({ models: await api.listChatModels() });
    } catch {
      /* 忽略:拿不到列表时回退为默认模型 */
    }
  },

  setModelRef: (ref) => set({ modelRef: ref }),

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
    // 会话记住上次使用的模型;该模型已不可用时保留当前选择
    const remembered = get().sessions.find((x) => x.id === id)?.modelRef ?? null;
    const usable = remembered && get().models.some((m) => m.ref === remembered) ? remembered : get().modelRef;
    set({
      sessionId: id,
      modelRef: usable,
      messages: msgs.map((m) => ({
        key: nextKey(), role: m.role, content: m.content, sources: m.sources ?? [], streaming: false,
        meta: m.role === "assistant" && m.model
          ? { model: m.model, provider: m.provider ?? "", elapsedMs: m.elapsedMs ?? 0, firstTokenMs: m.firstTokenMs }
          : undefined,
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
          patch((m) => ({ ...m, content: e.data.answer, streaming: false, meta: e.data.meta ?? undefined }));
          break;
      }
    };
    try {
      await api.chatAsk(get().sessionId, q, get().modelRef, onEvent);
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
