// @author ygw
// 顶部提示条的"已关闭"记忆:按提示标识保存在 localStorage,重启后依然保持关闭;
// 标识里带上具体内容(如模型名),所以换了模型或换了问题会重新提示

const KEY = "memflow.dismissedBanners";

/** 读取已关闭的提示标识集合(存储不可用或内容损坏时视为空) */
export function loadDismissed(): Set<string> {
  try {
    const raw = localStorage.getItem(KEY);
    const arr = raw ? (JSON.parse(raw) as unknown) : [];
    return new Set(Array.isArray(arr) ? arr.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

/** 记录某个提示已被关闭(最多保留最近 50 条,避免无限增长) */
export function saveDismissed(set: Set<string>): void {
  try {
    localStorage.setItem(KEY, JSON.stringify([...set].slice(-50)));
  } catch {
    /* 存储不可用时只在本次运行内生效 */
  }
}
