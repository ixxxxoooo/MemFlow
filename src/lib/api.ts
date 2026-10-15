// @author ixxxxoooo
// 前端到 Rust 后端的调用封装(统一错误为 Error 对象)

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AppStatus, CaptureResult, ChatEvent, ChatModelOption, ItemDetail, ItemFilter, Item, PullProgress,
  GroupSuggestion, Kb, MergeProposal, MoveResult, SingleProposal, RerankModel, Session, Settings, StoredMessage, TagCount,
} from "./types";

/** 调用后端命令,把字符串错误统一包装成 Error */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : (e as Error)?.message ?? String(e));
  }
}

export const api = {
  appStatus: () => call<AppStatus>("app_status"),
  ollamaStart: () => call<void>("ollama_start"),
  pullModel: (name: string, onProgress: (p: PullProgress) => void) => {
    const ch = new Channel<PullProgress>();
    ch.onmessage = onProgress;
    return call<void>("pull_model", { name, onProgress: ch });
  },
  /** 直连下载:绕过 Ollama 服务端的重定向检查(代理 fake-ip 环境) */
  pullModelDirect: (name: string, onProgress: (p: PullProgress) => void) => {
    const ch = new Channel<PullProgress>();
    ch.onmessage = onProgress;
    return call<void>("pull_model_direct", { name, onProgress: ch });
  },
  cancelPull: () => call<void>("cancel_pull"),
  /** 本地重排序模型及安装状态 */
  rerankModels: () => call<RerankModel[]>("rerank_models"),
  downloadRerankModel: (id: string, onProgress: (p: PullProgress) => void) => {
    const ch = new Channel<PullProgress>();
    ch.onmessage = onProgress;
    return call<void>("download_rerank_model", { id, onProgress: ch });
  },
  /** 保存云端 API Key 到系统钥匙串,空串表示清除 */
  setLlmApiKey: (key: string, profileId: string | null = null) =>
    call<void>("set_llm_api_key", { key, profileId }),
  /** 取回已保存的 API Key 明文(眼睛按钮用;会触发钥匙串授权) */
  revealApiKey: (profileId: string | null = null) => call<string>("reveal_api_key", { profileId }),
  /** 重新读取钥匙串里的密钥(授权被拒绝后用它重新触发授权弹窗) */
  retryKeyAccess: (profileId: string | null = null) => call<void>("retry_key_access", { profileId }),
  /** 测试云端连接;key 为空时使用已保存的密钥 */
  testLlmConnection: (baseUrl: string, model: string, key: string | null, profileId: string | null = null) =>
    call<string>("test_llm_connection", { baseUrl, model, key, profileId }),
  /** 测试云端向量/视觉模型;profileId 为空表示默认云端配置,key 为空时使用已保存的密钥 */
  testCloudModel: (kind: "embed" | "vision", profileId: string | null, model: string, key: string | null) =>
    call<string>("test_cloud_model", { kind, profileId, model, key }),
  /** 对话可选的模型(本地已安装 + 已配置密钥的云端) */
  /** 预热向量模型与所选本地模型,减少首次提问等待 */
  warmChat: (modelRef: string | null) => call<void>("warm_chat", { modelRef }),
  listChatModels: () => call<ChatModelOption[]>("list_chat_models"),
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (settings: Settings) => call<void>("update_settings", { settings }),
  captureSubmit: (text: string | null, paths: string[], kbId: number | null = null) =>
    call<CaptureResult>("capture_submit", { input: { text, paths, kbId } }),
  listKbs: () => call<Kb[]>("list_kbs"),
  createKb: (name: string, description: string | null = null) => call<Kb>("create_kb", { name, description }),
  renameKb: (id: number, name: string, description: string | null = null) =>
    call<void>("rename_kb", { id, name, description }),
  /** mode:moveToDefault 转移到默认知识库 / cascade 连同记录一起删除 */
  deleteKb: (id: number, mode: "moveToDefault" | "cascade") => call<number>("delete_kb", { id, mode }),
  moveItems: (ids: number[], kbId: number) => call<MoveResult>("move_items", { ids, kbId }),
  /** 把剪贴板里的二进制文件暂存到后端,返回暂存路径 */
  async stageFile(file: File): Promise<string> {
    const safe = file.name.replace(/[^A-Za-z0-9._-]/g, "_") || "paste.png";
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      return await invoke<string>("stage_file", bytes, { headers: { "x-filename": safe } });
    } catch (e) {
      throw new Error(typeof e === "string" ? e : String(e));
    }
  },
  listItems: (filter: ItemFilter) => call<Item[]>("list_items", { filter }),
  getItem: (id: number) => call<ItemDetail>("get_item", { id }),
  listTags: () => call<TagCount[]>("list_tags"),
  updateItemContent: (id: number, content: string) => call<void>("update_item_content", { id, content }),
  deleteItems: (ids: number[]) => call<number>("delete_items", { ids }),
  reindex: (ids: number[] | null) => call<number>("reindex", { ids }),
  reindexAi: (ids: number[]) => call<number>("reindex_ai", { ids }),
  retryFailed: (ids: number[] | null) => call<number>("retry_failed", { ids }),
  cancelItem: (id: number) => call<void>("cancel_item", { id }),
  chatAsk: (sessionId: number | null, question: string, modelRef: string | null, kbIds: number[], onEvent: (e: ChatEvent) => void) => {
    const ch = new Channel<ChatEvent>();
    ch.onmessage = onEvent;
    return call<void>("chat_ask", { sessionId, question, modelRef, kbIds, onEvent: ch });
  },
  chatCancel: () => call<void>("chat_cancel"),
  listSessions: () => call<Session[]>("list_sessions"),
  getSessionMessages: (sessionId: number) => call<StoredMessage[]>("get_session_messages", { sessionId }),
  deleteSession: (sessionId: number) => call<void>("delete_session", { sessionId }),
  /** AI 整理:只生成方案供预览,不写入 */
  organizePropose: (id: number, modelRef: string | null) => call<SingleProposal>("organize_propose", { id, modelRef }),
  organizeApply: (id: number, title: string, content: string, baseUpdatedAt: number) =>
    call<void>("organize_apply", { id, title, content, baseUpdatedAt }),
  organizeUndo: (id: number) => call<void>("organize_undo", { id }),
  /** 按向量相似度给出可归并的碎片分组(不调用模型) */
  organizeSuggest: (kbId: number | null) => call<GroupSuggestion[]>("organize_suggest", { options: { kbId } }),
  /** "可整理"提示:记录 id -> 高度相似的碎片 id */
  organizeHints: (ids: number[]) => call<Record<string, number[]>>("organize_hints", { ids }),
  organizeMergePropose: (ids: number[], modelRef: string | null) =>
    call<MergeProposal>("organize_merge_propose", { ids, modelRef }),
  organizeMergeApply: (ids: number[], title: string, content: string) =>
    call<number>("organize_merge_apply", { ids, title, content }),
  fixMermaid: (code: string, error: string) => call<string>("fix_mermaid", { code, error }),
  writeExport: (path: string, dataBase64: string) => call<void>("write_export", { path, dataBase64 }),
  exportBackup: (destDir: string) => call<string>("export_backup", { destDir }),
  showMain: () => call<void>("show_main"),
  showCapsule: () => call<void>("show_capsule"),
  hideCapsule: () => call<void>("hide_capsule"),
  resizeCapsule: (height: number) => call<void>("resize_capsule", { height }),
  notifyToast: (message: string) => call<void>("notify_toast", { message }),
  /** 把前端错误写入后端日志文件 */
  logFrontend: (level: "error" | "warn" | "info", message: string) =>
    call<void>("log_frontend", { level, message }),
  /** 打开日志目录,返回目录路径 */
  openLogDir: () => call<string>("open_log_dir"),
};
