// @author ygw
// Mermaid 校验与"自动修复"流程(纯函数,依赖通过参数注入以便测试)

/** 去掉模型可能带上的代码围栏与 mermaid 语言标记 */
export function normalizeMermaid(code: string): string {
  let c = code.trim();
  const m = c.match(/^```(?:mermaid)?\s*\n([\s\S]*?)\n?```$/);
  if (m) c = m[1].trim();
  if (c.startsWith("mermaid\n")) c = c.slice(8).trim();
  return c;
}

export interface RepairOutcome {
  /** 是否最终渲染成功 */
  ok: boolean;
  /** 最终使用的代码 */
  code: string;
  /** 成功时的 SVG */
  svg?: string;
  /** 失败时的最后一次错误 */
  error?: string;
  /** 实际发起的修复次数 */
  repairs: number;
}

/**
 * 渲染 Mermaid;失败则把报错回喂模型修复,最多 maxRepairs 次
 * @param code 初始代码
 * @param render 渲染函数,失败抛异常
 * @param fix 修复函数(返回新代码),失败抛异常
 * @param maxRepairs 最大修复次数
 */
export async function renderWithRepair(
  code: string,
  render: (code: string) => Promise<string>,
  fix: (code: string, error: string) => Promise<string>,
  maxRepairs = 2,
): Promise<RepairOutcome> {
  let current = normalizeMermaid(code);
  let lastError = "";
  for (let i = 0; i <= maxRepairs; i++) {
    try {
      const svg = await render(current);
      return { ok: true, code: current, svg, repairs: i };
    } catch (e) {
      lastError = e instanceof Error ? e.message : String(e);
      if (i === maxRepairs) break;
      try {
        current = normalizeMermaid(await fix(current, lastError));
      } catch (fe) {
        lastError = `${lastError}(自动修复失败:${fe instanceof Error ? fe.message : String(fe)})`;
        return { ok: false, code: current, error: lastError, repairs: i };
      }
    }
  }
  return { ok: false, code: current, error: lastError, repairs: maxRepairs };
}

/**
 * 给异步任务加超时;超时则以错误拒绝(原任务仍会跑完,但结果被丢弃)
 *
 * 参数: p 任务;ms 毫秒;message 超时提示
 */
export function withTimeout<T>(p: Promise<T>, ms: number, message: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const t = setTimeout(() => reject(new Error(message)), ms);
    p.then(
      (v) => { clearTimeout(t); resolve(v); },
      (e) => { clearTimeout(t); reject(e); },
    );
  });
}
