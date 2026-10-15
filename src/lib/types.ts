// @author ygw
// 与 Rust 后端一一对应的类型定义

export type ItemKind = "note" | "image" | "document";
export type ItemStatus = "pending" | "processing" | "done" | "archived_only" | "failed";

/** 知识库 */
export interface Kb {
  id: number;
  name: string;
  description: string;
  createdAt: number;
  /** 库内记录数 */
  itemCount: number;
}

/** 转移记录的结果 */
export interface MoveResult {
  moved: number[];
  /** 目标库已有相同内容而被跳过 */
  skipped: number[];
}

export interface Item {
  id: number;
  kind: ItemKind;
  /** 所属知识库 */
  kbId: number;
  /** 文件夹导入时的来源路径(文件夹名/相对路径) */
  sourcePath: string | null;
  title: string | null;
  content: string;
  contentLength: number;
  attachmentPath: string | null;
  originalName: string | null;
  mime: string | null;
  status: ItemStatus;
  error: string | null;
  createdAt: number;
  updatedAt: number;
  summary: string | null;
  category: string | null;
  tags: string[];
  entities: string[];
  visionText: string | null;
  /** 本机 OCR 识别出的图中文字(图片) */
  ocrText: string | null;
  /** 是否由文档内嵌/引用自动导入的图片 */
  embedded: boolean;
  metaModel: string | null;
  promptVersion: number | null;
  chunkCount: number;
}

export interface Chunk {
  id: number;
  seq: number;
  text: string;
  heading: string | null;
  docSummary: string | null;
  embedModel: string | null;
  /** 所在页码(PDF) */
  page: number | null;
  /** 该切片里出现的图片 */
  images: ImageRef[];
}

/** 切片/来源关联的图片 */
export interface ImageRef {
  id: number;
  attachmentPath: string | null;
  name: string | null;
}

/** 文档引用的一张图片 */
export interface LinkedImage {
  childId: number;
  /** 正文里的引用写法 */
  anchor: string;
  seq: number;
  originalName: string | null;
  attachmentPath: string | null;
  ocrText: string | null;
  visionDesc: string | null;
}

/** 引用某张图片的文档 */
export interface ParentRef {
  id: number;
  kind: ItemKind;
  title: string | null;
  originalName: string | null;
}

export interface ItemDetail {
  item: Item;
  chunks: Chunk[];
  /** 文档引用的图片(按出现顺序) */
  images: LinkedImage[];
  /** 图片被哪些文档引用 */
  parents: ParentRef[];
}

export interface ItemFilter {
  kind?: string;
  status?: string;
  tag?: string;
  search?: string;
  beforeId?: number;
  limit?: number;
  /** 限定知识库(省略 = 全部) */
  kbId?: number;
}

export interface Settings {
  ollamaUrl: string;
  embedModel: string;
  textModel: string;
  visionModel: string;
  visionEnabled: boolean;
  keepAlive: string;
  shortcut: string;
  scoreThreshold: number;
  topK: number;
  queryRewrite: boolean;
  numCtx: number;
  docAiEnrich: boolean;
  /** 文本模型来源:ollama 本地 / openai_compat 云端 */
  llmProvider: "ollama" | "openai_compat";
  llmPreset: string;
  llmBaseUrl: string;
  llmModel: string;
  /** 直连下载使用的模型仓库地址(可换镜像) */
  modelRegistry: string;
  /** 入库并发处理的文档数(0 = 自动) */
  importConcurrency: number;
  /** PDF 解析模式:auto 文本层+本机 OCR / vision 视觉模型逐页解析 */
  pdfParseMode: "auto" | "vision";
  /** 本地重排序开关(模型未下载时自动跳过) */
  rerankEnabled: boolean;
  /** 重排序模型标识 */
  rerankModel: string;
  /** 重排序模型下载镜像 */
  hfMirror: string;
  /** 额外的云端模型档案(对话里可切换) */
  llmProfiles: LlmProfile[];
}

/** 额外的云端模型档案;密钥单独存放在钥匙串 */
export interface LlmProfile {
  id: string;
  name: string;
  /** 预设标识,用于挑选图标 */
  preset: string;
  baseUrl: string;
  model: string;
}

/** 对话可选的模型 */
export interface ChatModelOption {
  /** 引用标识:ollama:<名> / cloud:default / cloud:<档案 id> */
  ref: string;
  provider: "ollama" | "openai_compat";
  label: string;
  model: string;
  preset: string | null;
}

/** 一次回答的模型与耗时信息 */
export interface AnswerMeta {
  model: string;
  provider: string;
  preset?: string | null;
  elapsedMs: number;
  firstTokenMs: number | null;
}

export interface RerankModel {
  id: string;
  label: string;
  /** 总下载大小(字节) */
  size: number;
  installed: boolean;
}

export interface Stats {
  items: number;
  notes: number;
  images: number;
  documents: number;
  chunks: number;
  staleChunks: number;
  pendingItems: number;
  failedItems: number;
}

export interface QueueStatus {
  pending: number;
  running: number;
  failed: number;
  currentItem: { id: number; kind: ItemKind; title: string | null } | null;
  blockedReason: string | null;
}

export interface ModelNeed {
  role: string;
  name: string;
  installed: boolean;
  required: boolean;
  /** 本地 Ollama 模型(云端模型为 false,不可下载) */
  local: boolean;
}

export interface AppStatus {
  ollamaRunning: boolean;
  ollamaVersion: string | null;
  installedModels: string[];
  models: ModelNeed[];
  stats: Stats;
  queue: QueueStatus;
  settings: Settings;
  dataDir: string;
  vectorCount: number;
  /** 是否已配置云端 API Key(密钥不会返回前端) */
  hasApiKey: boolean;
  /** 已保存密钥的额外档案 id */
  keyedProfiles: string[];
}

export interface TagCount {
  tag: string;
  count: number;
}

export interface Source {
  n: number;
  chunkId: number;
  itemId: number;
  kind: ItemKind;
  /** 来源所在知识库 */
  kbId: number;
  kbName: string | null;
  title: string | null;
  text: string;
  heading: string | null;
  /** 所在页码(PDF) */
  page: number | null;
  createdAt: number;
  attachmentPath: string | null;
  score: number;
  /** 该切片里出现的图片(文档插图) */
  images: ImageRef[];
  /** 图片命中时的来源文档,如「来自《A》」 */
  via: string | null;
}

export interface Session {
  id: number;
  title: string;
  updatedAt: number;
  /** 该会话最近一次使用的模型引用 */
  modelRef: string | null;
  /** 该会话限定的知识库范围(空 = 全部) */
  kbIds: number[];
}

export interface StoredMessage {
  id: number;
  role: "user" | "assistant";
  content: string;
  sources: Source[] | null;
  createdAt: number;
  model: string | null;
  provider: string | null;
  elapsedMs: number | null;
  firstTokenMs: number | null;
}

export type ChatEvent =
  | { event: "session"; data: { sessionId: number } }
  | { event: "stage"; data: { stage: "rewrite" | "retrieve" | "rerank" | "generate" } }
  | { event: "sources"; data: { sources: Source[] } }
  | { event: "token"; data: { text: string } }
  | { event: "done"; data: { messageId: number; answer: string; stopped: boolean; meta: AnswerMeta | null } };

export interface PullProgress {
  status: string;
  total: number;
  completed: number;
}

export interface CaptureResult {
  created: number[];
  duplicates: number[];
  errors: string[];
  /** 被跳过内容的汇总行(按原因合并) */
  skipped: string[];
  skippedCount: number;
  /** created 中由文档引用/内嵌自动导入并关联的图片数 */
  linkedImages: number;
}
