// @author ygw
// 知识库状态:库列表与增删改查、时间流的库筛选(持久化)。
// 提问范围由 chatStore.kbIds 负责;导入目标由调用方显式传入(档案库内的当前库,或默认库)。

import { create } from "zustand";
import { api } from "./api";
import type { Kb, MoveResult } from "./types";

const TIMELINE_KEY = "memflow.timelineKb";

/** 读取持久化的时间流库筛选 */
function readTimeline(): number | null {
  try {
    const v = localStorage.getItem(TIMELINE_KEY);
    const n = v == null ? NaN : Number(v);
    return Number.isInteger(n) ? n : null;
  } catch {
    return null;
  }
}

function writeTimeline(id: number | null) {
  try {
    if (id == null) localStorage.removeItem(TIMELINE_KEY);
    else localStorage.setItem(TIMELINE_KEY, String(id));
  } catch {
    /* 忽略:存储不可用时只在本次运行内生效 */
  }
}

interface KbState {
  kbs: Kb[];
  /** 时间流的库筛选;null 表示「全部知识库」 */
  timelineKb: number | null;
  load: () => Promise<void>;
  setTimelineKb: (id: number | null) => void;
  create: (name: string, description?: string) => Promise<Kb>;
  /** 修改名称与描述 */
  update: (id: number, name: string, description?: string) => Promise<void>;
  remove: (id: number, mode: "moveToDefault" | "cascade") => Promise<number>;
  move: (ids: number[], kbId: number) => Promise<MoveResult>;
}

export const useKb = create<KbState>((set, get) => ({
  kbs: [],
  timelineKb: readTimeline(),

  load: async () => {
    try {
      const kbs = await api.listKbs();
      const { timelineKb } = get();
      // 筛选的库已被删除:回到「全部」
      const keep = timelineKb != null && kbs.some((k) => k.id === timelineKb) ? timelineKb : null;
      if (keep !== timelineKb) writeTimeline(keep);
      set({ kbs, timelineKb: keep });
    } catch {
      /* 忽略:列表拿不到时沿用旧数据 */
    }
  },

  setTimelineKb: (id) => {
    writeTimeline(id);
    set({ timelineKb: id });
  },

  create: async (name, description = "") => {
    const kb = await api.createKb(name, description);
    await get().load();
    return kb;
  },

  update: async (id, name, description) => {
    await api.renameKb(id, name, description);
    await get().load();
  },

  remove: async (id, mode) => {
    const n = await api.deleteKb(id, mode);
    await get().load();
    return n;
  },

  move: async (ids, kbId) => {
    const r = await api.moveItems(ids, kbId);
    await get().load();
    return r;
  },
}));
