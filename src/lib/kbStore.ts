// @author ygw
// 知识库状态:列表、侧栏当前选择、最近一次导入所选(持久化到 localStorage,胶囊窗口共享)

import { create } from "zustand";
import { api } from "./api";
import { importTargetKb } from "./kbScope";
import type { Kb, MoveResult } from "./types";

const CURRENT_KEY = "memflow.currentKb";
const LAST_IMPORT_KEY = "memflow.lastImportKb";

/** 读取持久化的知识库 id */
function readId(key: string): number | null {
  try {
    const v = localStorage.getItem(key);
    const n = v == null ? NaN : Number(v);
    return Number.isInteger(n) ? n : null;
  } catch {
    return null;
  }
}

/** 写入持久化的知识库 id(null 表示清除) */
function writeId(key: string, id: number | null) {
  try {
    if (id == null) localStorage.removeItem(key);
    else localStorage.setItem(key, String(id));
  } catch {
    /* 忽略:存储不可用时只在本次运行内生效 */
  }
}

interface KbState {
  kbs: Kb[];
  /** 侧栏当前选择;null 表示「全部知识库」 */
  current: number | null;
  /** 最近一次导入所选的知识库 */
  lastImport: number | null;
  load: () => Promise<void>;
  setCurrent: (id: number | null) => void;
  /** 记住导入目标(胶囊与导入入口使用) */
  rememberImport: (id: number) => void;
  /** 当前应导入到的知识库 id */
  importTarget: () => number;
  create: (name: string) => Promise<Kb>;
  rename: (id: number, name: string) => Promise<void>;
  remove: (id: number, mode: "moveToDefault" | "cascade") => Promise<number>;
  move: (ids: number[], kbId: number) => Promise<MoveResult>;
}

export const useKb = create<KbState>((set, get) => ({
  kbs: [],
  current: readId(CURRENT_KEY),
  lastImport: readId(LAST_IMPORT_KEY),

  load: async () => {
    try {
      const kbs = await api.listKbs();
      const { current, lastImport } = get();
      // 当前选择的库已被删除:回到「全部」
      const keepCurrent = current != null && kbs.some((k) => k.id === current) ? current : null;
      if (keepCurrent !== current) writeId(CURRENT_KEY, keepCurrent);
      set({ kbs, current: keepCurrent, lastImport: lastImport != null && kbs.some((k) => k.id === lastImport) ? lastImport : null });
    } catch {
      /* 忽略:列表拿不到时沿用旧数据 */
    }
  },

  setCurrent: (id) => {
    writeId(CURRENT_KEY, id);
    set({ current: id });
  },

  rememberImport: (id) => {
    writeId(LAST_IMPORT_KEY, id);
    set({ lastImport: id });
  },

  importTarget: () => importTargetKb(get().current, get().lastImport, get().kbs),

  create: async (name) => {
    const kb = await api.createKb(name);
    await get().load();
    return kb;
  },

  rename: async (id, name) => {
    await api.renameKb(id, name);
    await get().load();
  },

  remove: async (id, mode) => {
    const n = await api.deleteKb(id, mode);
    if (get().current === id) get().setCurrent(null);
    await get().load();
    return n;
  },

  move: async (ids, kbId) => {
    const r = await api.moveItems(ids, kbId);
    await get().load();
    return r;
  },
}));
