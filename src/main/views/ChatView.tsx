// @author ygw
// AI 问答视图:对话流 + 流式输出 + 来源角标 + 历史会话

import { useEffect, useRef, useState } from "react";
import { confirmDialog } from "../../lib/confirm";
import { fmtDate } from "../../lib/format";
import { useChat } from "../../lib/chatStore";
import { useApp } from "../../lib/store";
import type { Source } from "../../lib/types";
import { HistoryIcon, PlusIcon, SendIcon, SparkleIcon, StopIcon, TrashIcon } from "../components/icons";
import { Markdown } from "../components/Markdown";

const SUGGESTIONS = [
  "最近我都记录了哪些事?帮我按时间梳理一下",
  "帮我总结一下知识库里最近的工作进展",
  "我之前记过哪些待办或需要跟进的事情?",
];

export function ChatView() {
  const { messages, busy, send, stop, sessions, loadSessions, openSession, newSession, removeSession, sessionId } = useChat();
  const { openDrawer, openDocModal, status, toast } = useApp();
  const [input, setInput] = useState("");
  const [showHistory, setShowHistory] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const taRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => { void loadSessions(); }, [loadSessions]);

  // 流式输出时自动贴底;用户上滑查看历史时不打扰
  useEffect(() => {
    if (stick.current) endRef.current?.scrollIntoView({ block: "end" });
  }, [messages]);

  const submit = async () => {
    const q = input.trim();
    if (!q || busy) return;
    setInput("");
    stick.current = true;
    if (taRef.current) taRef.current.style.height = "auto";
    await send(q);
  };

  const onCite = (s: Source) => (s.kind === "document" ? openDocModal(s.itemId, s.chunkId) : openDrawer(s.itemId, s.chunkId));
  const empty = messages.length === 0;

  return (
    <div className="relative flex h-full flex-col">
      <div data-tauri-drag-region className="flex h-[52px] shrink-0 items-center justify-between px-6">
        <div className="text-[13px] font-medium text-[var(--fg-dim)]" data-tauri-drag-region>
          {sessionId ? sessions.find((s) => s.id === sessionId)?.title ?? "对话" : "新对话"}
        </div>
        <div className="flex items-center gap-1">
          <button className="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-[var(--fg-dim)] hover:bg-[var(--bg-soft)]" onClick={() => setShowHistory((v) => !v)}>
            <HistoryIcon size={14} />历史
          </button>
          <button className="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-[var(--fg-dim)] hover:bg-[var(--bg-soft)] disabled:opacity-40" disabled={busy} onClick={newSession}>
            <PlusIcon size={14} />新对话
          </button>
        </div>
      </div>

      {showHistory && (
        <div className="pop-in absolute right-6 top-[48px] z-20 max-h-80 w-72 overflow-y-auto rounded-xl border border-[var(--line)] bg-[var(--bg-elev)] p-1.5 shadow-xl">
          {sessions.length === 0 && <div className="p-3 text-xs text-[var(--fg-dim)]">还没有历史对话</div>}
          {sessions.map((s) => (
            <div key={s.id} className={`group flex items-center gap-2 rounded-lg px-2.5 py-2 text-[13px] hover:bg-[var(--bg-soft)] ${s.id === sessionId ? "bg-[var(--accent-soft)]" : ""}`}>
              <button className="min-w-0 flex-1 text-left" onClick={async () => { try { await openSession(s.id); setShowHistory(false); } catch (e) { toast((e as Error).message, "err"); } }}>
                <div className="truncate">{s.title}</div>
                <div className="text-[10px] text-[var(--fg-faint)]">{fmtDate(s.updatedAt)}</div>
              </button>
              <button className="opacity-0 group-hover:opacity-100" onClick={async () => { if (await confirmDialog("删除这段对话?")) await removeSession(s.id); }}>
                <TrashIcon size={13} className="text-[var(--fg-dim)] hover:text-[var(--err)]" />
              </button>
            </div>
          ))}
        </div>
      )}

      <div
        ref={scroller}
        className="flex-1 overflow-y-auto px-6"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        <div className="mx-auto max-w-[760px] pb-6">
          {empty ? (
            <div className="flex flex-col items-center pt-[12vh] text-center">
              <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--accent-soft)] text-[var(--accent)]"><SparkleIcon size={26} /></div>
              <h1 className="text-xl font-semibold">问问你的第二大脑</h1>
              <p className="mt-2 text-sm text-[var(--fg-dim)]">
                已收录 {status?.stats.items ?? 0} 条内容。回答只依据你的记录,并标注可点击的来源。
              </p>
              <div className="mt-6 flex w-full max-w-md flex-col gap-2">
                {SUGGESTIONS.map((s) => (
                  <button key={s} className="rounded-xl border border-[var(--line)] px-4 py-2.5 text-left text-[13px] text-[var(--fg-dim)] hover:border-[var(--accent)] hover:text-[var(--fg)]" onClick={() => { setInput(s); taRef.current?.focus(); }}>
                    {s}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            messages.map((m) =>
              m.role === "user" ? (
                <div key={m.key} className="pop-in mt-5 flex justify-end">
                  <div className="selectable max-w-[85%] whitespace-pre-wrap rounded-2xl rounded-br-md bg-[var(--bubble)] px-4 py-2.5 text-[14.5px] leading-relaxed text-[var(--fg)]">{m.content}</div>
                </div>
              ) : (
                <div key={m.key} className="pop-in mt-5">
                  {m.content ? (
                    <Markdown text={m.content} sources={m.sources} streaming={m.streaming} onCite={onCite} />
                  ) : null}
                  {m.streaming && !m.content && <div className="text-sm text-[var(--fg-dim)]">正在检索并思考<span className="typing-dot" /></div>}
                  {m.error && <div className="mt-2 rounded-lg bg-[var(--err)]/10 px-3 py-2 text-[13px] text-[var(--err)]">{m.error}</div>}
                  {m.sources.length > 0 && !m.streaming && (
                    <div className="mt-3 flex flex-wrap items-center gap-1.5 text-[11.5px]">
                      <span className="text-[var(--fg-faint)]">引用来源</span>
                      {m.sources.map((s) => (
                        <button key={s.n} onClick={() => onCite(s)} className="flex max-w-[200px] items-center gap-1 rounded-full border border-[var(--line)] px-2 py-0.5 hover:border-[var(--accent)] hover:text-[var(--accent)]">
                          <span className="font-semibold text-[var(--accent)]">{s.n}</span>
                          <span className="truncate">{s.title ?? (s.kind === "image" ? "图片" : "记录")}{s.page ? ` p.${s.page}` : ""}</span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              ),
            )
          )}
          <div ref={endRef} />
        </div>
      </div>

      <div className="px-6 pb-5">
        <div className="mx-auto flex max-w-[760px] items-end gap-2 rounded-2xl border border-[var(--line)] bg-[var(--bg-elev)] p-2 shadow-sm focus-within:border-[var(--accent)]">
          <textarea
            ref={taRef}
            rows={1}
            value={input}
            placeholder="向你的知识库提问…(Enter 发送,Shift+Enter 换行)"
            className={`max-h-40 flex-1 resize-none bg-transparent px-2.5 py-2 text-[14.5px] outline-none ${busy ? "caret-transparent" : ""}`}
            onChange={(e) => {
              setInput(e.target.value);
              e.target.style.height = "auto";
              e.target.style.height = `${Math.min(e.target.scrollHeight, 160)}px`;
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void submit();
              }
            }}
          />
          {busy ? (
            <button className="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--bg-soft)] hover:bg-[var(--line)]" onClick={() => void stop()} title="停止生成"><StopIcon size={14} /></button>
          ) : (
            <button className="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--accent)] text-[var(--on-accent)] disabled:opacity-35" disabled={!input.trim()} onClick={() => void submit()}><SendIcon size={16} /></button>
          )}
        </div>
      </div>
    </div>
  );
}
