// @author ygw
// 删除条目后抽屉/弹窗的收口逻辑

import { beforeEach, describe, expect, it, vi } from "vitest";

// 避免加载 Tauri 运行时
vi.mock("./api", () => ({ api: {} }));

import { isMissingItemError } from "./itemErrors";
import { useApp } from "./store";

describe("itemsDeleted", () => {
  beforeEach(() => {
    useApp.setState({ drawer: null, docModal: null, dataVersion: 0 });
  });

  it("删除正在展示的条目会关闭抽屉并刷新数据", () => {
    useApp.setState({ drawer: { itemId: 7 }, docModal: { itemId: 7 } });
    useApp.getState().itemsDeleted([7]);
    const s = useApp.getState();
    expect(s.drawer).toBeNull();
    expect(s.docModal).toBeNull();
    expect(s.dataVersion).toBe(1);
  });

  it("删除其他条目不影响当前抽屉", () => {
    useApp.setState({ drawer: { itemId: 7 } });
    useApp.getState().itemsDeleted([8]);
    expect(useApp.getState().drawer).toEqual({ itemId: 7 });
  });
});

describe("isMissingItemError", () => {
  it("识别记录不存在", () => {
    expect(isMissingItemError("参数无效: 记录不存在")).toBe(true);
    expect(isMissingItemError("数据库错误")).toBe(false);
  });
});
