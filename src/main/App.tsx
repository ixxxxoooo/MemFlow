// @author ygw
// 主工作台:三栏布局(左导航 / 中工作区 / 右溯源抽屉)

import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { skipDetailLine, summarizeCapture } from "../lib/importSummary";
import { useKb } from "../lib/kbStore";
import { useApp } from "../lib/store";
import type { QueueStatus } from "../lib/types";
import { UploadIcon } from "./components/icons";
import { DocModal } from "./components/DocModal";
import { ItemDrawer } from "./components/ItemDrawer";
import { OrganizeDialog } from "./components/OrganizeDialog";
import { Sidebar } from "./components/Sidebar";
import { StatusBanner } from "./components/StatusBanner";
import { Toasts } from "./components/Toasts";
import { ChatView } from "./views/ChatView";
import { DocsView } from "./views/DocsView";
import { SettingsView } from "./views/SettingsView";
import { TimelineView } from "./views/TimelineView";

export function App() {
  const { view, drawer, refreshStatus, setQueue, bumpData, itemsDeleted, toast, closeDrawer, closeDocModal } = useApp();
  const [dragging, setDragging] = useState(false);
  const loadKbs = useKb((s) => s.load);

  // 订阅后端事件;Ollama 状态变化由后端推送,定时轮询只作兜底(30 秒)
  useEffect(() => {
    void refreshStatus();
    const timer = setInterval(() => void refreshStatus(), 30000);
    const unlistens: Promise<() => void>[] = [
      listen<QueueStatus>("queue-status", (e) => { setQueue(e.payload); }),
      listen("ollama-status", () => { void refreshStatus(); }),
      listen("item-updated", () => { bumpData(); }),
      listen<{ id: number }>("item-deleted", (e) => { itemsDeleted([e.payload.id]); }),
      listen("kb-changed", () => { void loadKbs(); bumpData(); }),
    ];
    return () => {
      clearInterval(timer);
      unlistens.forEach((p) => p.then((f) => f()));
    };
  }, [refreshStatus, setQueue, bumpData, itemsDeleted, loadKbs]);

  // 启动时载入知识库;数据变化(导入/删除/转移)后刷新各库记录数
  useEffect(() => { void loadKbs(); }, [loadKbs]);
  const dataVersion = useApp((s) => s.dataVersion);
  useEffect(() => { void loadKbs(); }, [dataVersion, loadKbs]);

  // 队列从忙到空闲时刷新统计
  const busy = useApp((s) => (s.queue?.pending ?? 0) + (s.queue?.running ?? 0));
  useEffect(() => { if (busy === 0) void refreshStatus(); }, [busy, refreshStatus]);

  // 把文件拖进主窗口即导入
  useEffect(() => {
    const p = getCurrentWebview().onDragDropEvent(async (e) => {
      const t = e.payload.type;
      if (t === "enter" || t === "over") setDragging(true);
      else if (t === "leave") setDragging(false);
      else if (t === "drop") {
        setDragging(false);
        try {
          const kb = useKb.getState();
          const target = kb.importTarget();
          kb.rememberImport(target);
          const targetName = kb.kbs.find((k) => k.id === target)?.name ?? "默认知识库";
          const r = await api.captureSubmit(null, e.payload.paths, target);
          const sum = summarizeCapture(r, targetName);
          toast(`${sum.text}${r.created.length ? ",后台解析中" : ""}${sum.detail.length ? `(跳过:${skipDetailLine(sum.detail)})` : ""}`, sum.kind);
          if (r.errors.length) toast(r.errors.join(";"), "err");
          bumpData();
          void refreshStatus();
        } catch (err) {
          toast((err as Error).message, "err");
        }
      }
    });
    return () => { p.then((f) => f()); };
  }, [toast, bumpData, refreshStatus]);

  // 切换视图时关闭抽屉,避免残留无关详情
  useEffect(() => { closeDrawer(); closeDocModal(); }, [view, closeDrawer, closeDocModal]);

  return (
    <div className="flex h-full">
      <Sidebar />
      <main className="relative flex min-w-0 flex-1 flex-col bg-[var(--bg)]">
        <StatusBanner />
        <div className="min-h-0 flex-1">
          {view === "chat" && <ChatView />}
          {view === "timeline" && <TimelineView />}
          {view === "docs" && <DocsView />}
          {view === "settings" && <SettingsView />}
        </div>
        {dragging && (
          <div className="pointer-events-none absolute inset-3 z-40 flex items-center justify-center rounded-2xl border-2 border-dashed border-[var(--accent)] bg-[var(--accent-soft)] backdrop-blur-sm">
            <div className="flex items-center gap-2 text-sm font-medium text-[var(--accent)]"><UploadIcon size={18} />松手即可导入图片 / 文档 / 文件夹</div>
          </div>
        )}
      </main>
      {drawer && view !== "settings" && <ItemDrawer />}
      <DocModal />
      <OrganizeDialog />
      <Toasts />
    </div>
  );
}
