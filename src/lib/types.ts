// @author ygw
// 与 Rust 后端一一对应的类型定义

export type ItemKind = "note" | "image" | "document";
export type ItemStatus = "pending" | "processing" | "done" | "archived_only" | "failed";

export interface Item {
  id: number;
  kind: ItemKind;
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
}

export interface ItemDetail {
  item: Item;
  chunks: Chunk[];
}

export interface ItemFilter {
  kind?: string;
  status?: string;
  tag?: string;
  search?: string;
  beforeId?: number;
  limit?: number;
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
  title: string | null;
  text: string;
  heading: string | null;
  createdAt: number;
  attachmentPath: string | null;
  score: number;
}

export interface Session {
  id: number;
  title: string;
  updatedAt: number;
}

export interface StoredMessage {
  id: number;
  role: "user" | "assistant";
  content: string;
  sources: Source[] | null;
  createdAt: number;
}

export type ChatEvent =
  | { event: "session"; data: { sessionId: number } }
  | { event: "sources"; data: { sources: Source[] } }
  | { event: "token"; data: { text: string } }
  | { event: "done"; data: { messageId: number; answer: string; stopped: boolean } };

export interface PullProgress {
  status: string;
  total: number;
  completed: number;
}

export interface CaptureResult {
  created: number[];
  duplicates: number[];
  errors: string[];
}
