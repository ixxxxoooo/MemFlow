// @author ygw
// 左栏:导航 + Ollama 状态呼吸灯 + 后台队列状态

import { useApp, type View } from "../../lib/store";
import { ChatIcon, LibraryIcon, SettingsIcon, SparkleIcon, TimelineIcon } from "./icons";

const NAV: { id: View; label: string; icon: typeof ChatIcon }[] = [
  { id: "chat", label: "AI 问答", icon: ChatIcon },
  { id: "timeline", label: "记忆时间流", icon: TimelineIcon },
  { id: "docs", label: "知识档案库", icon: LibraryIcon },
];

export function Sidebar() {
  const { view, setView, status, queue } = useApp();
  const running = status?.ollamaRunning ?? false;
  const textModel = status?.settings.textModel ?? "";
  const missing = status?.models.filter((m) => m.required && !m.installed) ?? [];

  let dot = "var(--ok)";
  let label = `Ollama 运行中 · ${textModel}`;
  if (!status) {
    dot = "var(--fg-faint)";
    label = "正在检测 Ollama…";
  } else if (!running) {
    dot = "var(--err)";
    label = "Ollama 未运行";
  } else if (missing.length > 0) {
    dot = "var(--warn)";
    label = `缺少模型:${missing.map((m) => m.name).join("、")}`;
  }

  let queueText: string | null = null;
  if (queue) {
    const waiting = queue.pending + queue.running;
    if (queue.currentItem) {
      queueText = `正在解析:${queue.currentItem.title ?? (queue.currentItem.kind === "image" ? "图片" : "记录")}${waiting > 1 ? `(还有 ${waiting - 1} 项)` : ""}`;
    } else if (waiting > 0) {
      queueText = queue.blockedReason ? `${waiting} 项等待中 · ${queue.blockedReason}` : `${waiting} 项排队中`;
    }
  }

  return (
    <aside className="flex h-full w-[212px] shrink-0 flex-col" style={{ background: "var(--sidebar-tint)" }}>
      <div data-tauri-drag-region className="h-[52px] shrink-0" />
      <div className="flex items-center gap-2 px-5 pb-3" data-tauri-drag-region>
        <SparkleIcon size={16} className="text-[var(--accent)]" />
        <span className="text-[15px] font-semibold tracking-tight">MemFlow</span>
      </div>
      <nav className="flex flex-col gap-0.5 px-2.5">
        {NAV.map((n) => (
          <button
            key={n.id}
            onClick={() => setView(n.id)}
            className={`flex items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13.5px] transition-colors ${
              view === n.id ? "bg-[var(--accent-soft)] font-medium text-[var(--accent)]" : "text-[var(--fg)] hover:bg-black/5 dark:hover:bg-white/5"
            }`}
          >
            <n.icon size={16} />
            {n.label}
          </button>
        ))}
      </nav>
      <div className="flex-1" />
      <div className="px-2.5">
        <button
          onClick={() => setView("settings")}
          className={`mb-2 flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13.5px] ${
            view === "settings" ? "bg-[var(--accent-soft)] font-medium text-[var(--accent)]" : "hover:bg-black/5 dark:hover:bg-white/5"
          }`}
        >
          <SettingsIcon size={16} />
          设置
        </button>
      </div>
      <div className="mx-3 mb-3 rounded-xl border border-[var(--line)] bg-[var(--bg-elev)]/60 p-2.5 text-[11.5px] leading-snug">
        <div className="flex items-start gap-2">
          <span className={`mt-[3px] h-2 w-2 shrink-0 rounded-full ${running && missing.length === 0 ? "breathe" : ""}`} style={{ background: dot }} />
          <span className="text-[var(--fg-dim)]">{label}</span>
        </div>
        {queueText && (
          <div className="mt-1.5 flex items-start gap-2 text-[var(--fg-dim)]">
            <span className="spin mt-[3px] inline-block h-2.5 w-2.5 shrink-0 rounded-full border-[1.5px] border-[var(--accent)] border-t-transparent" />
            <span>{queueText}</span>
          </div>
        )}
        {queue && queue.failed > 0 && (
          <div className="mt-1.5 text-[var(--err)]">{queue.failed} 项处理失败</div>
        )}
      </div>
    </aside>
  );
}
