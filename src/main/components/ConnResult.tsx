// @author ygw
// 连接测试结果:成功显示绿色对号,失败显示红色叉与原因

import { CheckIcon, CloseIcon } from "./icons";

export interface ConnState {
  ok: boolean;
  msg: string;
}

export function ConnResult({ state }: { state: ConnState | null }) {
  if (!state) return null;
  return (
    <span
      role="status"
      data-testid="conn-result"
      className={`inline-flex min-w-0 items-center gap-1 text-xs ${state.ok ? "text-[var(--ok)]" : "text-[var(--err)]"}`}
      title={state.msg}
    >
      {state.ok ? <CheckIcon size={15} /> : <CloseIcon size={14} />}
      <span className="max-w-[260px] truncate">{state.ok ? "连接成功" : state.msg}</span>
    </span>
  );
}
