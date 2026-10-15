// @author ixxxxoooo
// AI 整理对话框:单条整理(对比预览)、归并预览(来源编号对照)、碎片建议列表。
// 所有方案都是"先预览、用户确认后才写入";云端模型会先提示隐私风险。

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api } from "../../lib/api";
import { useChat } from "../../lib/chatStore";
import { fmtDate, fmtTime } from "../../lib/format";
import { useKb } from "../../lib/kbStore";
import { briefName, organizeModel, pct, useOrganize } from "../../lib/organize";
import { useApp } from "../../lib/store";
import type { GroupSuggestion, MergeProposal, NoteBrief, SingleProposal } from "../../lib/types";
import { AlertIcon, CloseIcon, RefreshIcon, SparkleIcon } from "./icons";

type Phase = "gate" | "loading" | "ready" | "applying" | "error";

/** 生成方案的通用流程:本地模型直接开始,云端模型先弹出隐私提示 */
function useProposal<T>(run: (modelRef: string | null) => Promise<T>) {
  const settings = useApp((s) => s.status?.settings);
  const loadModels = useChat((s) => s.loadModels);
  const [phase, setPhase] = useState<Phase>("loading");
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState("");
  const [model, setModel] = useState<{ ref: string | null; label: string; cloud: boolean }>({ ref: null, label: "", cloud: false });
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const go = useCallback(async (ref: string | null) => {
    setPhase("loading");
    setError("");
    try {
      const r = await run(ref);
      if (!alive.current) return;
      setData(r);
      setPhase("ready");
    } catch (e) {
      if (!alive.current) return;
      setError((e as Error).message);
      setPhase("error");
    }
  }, [run]);

  /** 读取最新的模型选择后开始(云端则停在隐私提示) */
  const start = useCallback(async (force = false) => {
    if (useChat.getState().models.length === 0) await loadModels();
    const st = useChat.getState();
    const info = organizeModel(st.modelRef, st.models, settings);
    if (!alive.current) return;
    setModel(info);
    if (info.cloud && !force) setPhase("gate");
    else await go(info.ref);
  }, [go, loadModels, settings]);

  useEffect(() => { void start(); /* 只在打开时执行一次 */ // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { phase, setPhase, data, error, model, start, go };
}

/** 云端隐私提示 + 开始按钮 */
function CloudGate({ model, count, onGo }: { model: string; count: number; onGo: () => void }) {
  return (
    <div className="mx-auto mt-16 max-w-md rounded-2xl border border-[var(--line)] bg-[var(--bg-elev)] p-6 text-center">
      <div className="mx-auto mb-3 flex h-10 w-10 items-center justify-center rounded-full bg-[var(--err)]/10 text-[var(--err)]"><AlertIcon size={20} /></div>
      <div className="text-sm font-semibold">将把 {count} 条笔记的原文发送给云端模型</div>
      <p className="mt-2 text-xs leading-relaxed text-[var(--fg-dim)]">
        当前模型「{model}」运行在云端,笔记内容会离开本机。若包含敏感信息,请先在问答输入框旁切换成本地模型再来整理。
      </p>
      <button className="btn btn-primary mt-4" onClick={onGo}>我了解,继续整理</button>
    </div>
  );
}

/** 警告与不确定项 */
function Notices({ warnings, notes }: { warnings: string[]; notes: string[] }) {
  if (!warnings.length && !notes.length) return null;
  return (
    <div className="space-y-2">
      {warnings.length > 0 && (
        <div className="rounded-lg border border-[var(--err)]/30 bg-[var(--err)]/10 px-3 py-2 text-xs text-[var(--err)]">
          <div className="mb-0.5 font-medium">请注意(程序校验发现)</div>
          <ul className="list-disc space-y-0.5 pl-4">{warnings.map((w, i) => <li key={i}>{w}</li>)}</ul>
        </div>
      )}
      {notes.length > 0 && (
        <div className="rounded-lg border border-[var(--line)] bg-[var(--bg-soft)] px-3 py-2 text-xs text-[var(--fg-dim)]">
          <div className="mb-0.5 font-medium">模型标出需要你核对的地方</div>
          <ul className="list-disc space-y-0.5 pl-4">{notes.map((w, i) => <li key={i}>{w}</li>)}</ul>
        </div>
      )}
    </div>
  );
}

/** 对话框外壳 */
function Shell({ title, sub, onClose, busy, children, footer }: { title: string; sub?: ReactNode; onClose: () => void; busy: boolean; children: ReactNode; footer?: ReactNode }) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape" && !busy) onClose(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onClose, busy]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6 backdrop-blur-[2px]" onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
      <div className="pop-in flex h-full max-h-[780px] w-full max-w-[1000px] flex-col overflow-hidden rounded-2xl border border-[var(--line)] bg-[var(--bg)] shadow-2xl">
        <div className="flex items-center justify-between gap-4 border-b border-[var(--line)] px-5 py-3.5">
          <div className="flex min-w-0 items-center gap-2">
            <SparkleIcon size={16} className="shrink-0 text-[var(--accent)]" />
            <div className="truncate text-[15px] font-semibold">{title}</div>
            {sub && <div className="truncate text-xs text-[var(--fg-dim)]">{sub}</div>}
          </div>
          <button className="rounded-full p-1.5 text-[var(--fg-dim)] hover:bg-[var(--bg-soft)] disabled:opacity-40" disabled={busy} onClick={onClose} title="关闭 (Esc)"><CloseIcon size={15} /></button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer && <div className="flex items-center justify-end gap-2 border-t border-[var(--line)] px-5 py-3">{footer}</div>}
      </div>
    </div>
  );
}

function Loading({ text }: { text: string }) {
  return (
    <div className="flex h-full min-h-[240px] flex-col items-center justify-center gap-3 text-sm text-[var(--fg-dim)]">
      <span className="spin inline-block h-5 w-5 rounded-full border-2 border-[var(--accent)] border-t-transparent" />
      {text}
    </div>
  );
}

function ErrorBox({ text, onRetry }: { text: string; onRetry: () => void }) {
  return (
    <div className="mx-auto mt-16 max-w-md text-center">
      <div className="rounded-lg bg-[var(--err)]/10 p-3 text-xs text-[var(--err)]">{text}</div>
      <button className="btn btn-secondary mt-3" onClick={onRetry}><RefreshIcon size={13} />重试</button>
    </div>
  );
}

const fieldCls = "w-full rounded-lg border border-[var(--line)] bg-[var(--bg-soft)] p-2.5 text-[13px] leading-relaxed outline-none focus:border-[var(--accent)]";

/** 单条整理:左原文,右整理后(可编辑),确认后覆盖 */
function SingleView({ itemId, onClose }: { itemId: number; onClose: () => void }) {
  const { toast, bumpData, refreshStatus } = useApp();
  const run = useCallback((ref: string | null) => api.organizePropose(itemId, ref), [itemId]);
  const { phase, setPhase, data, error, model, start, go } = useProposal<SingleProposal>(run);
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  useEffect(() => { if (data) { setTitle(data.title); setContent(data.content); } }, [data]);

  const apply = async () => {
    if (!data) return;
    setPhase("applying");
    try {
      await api.organizeApply(itemId, title, content, data.baseUpdatedAt);
      toast("已整理并覆盖原文,可在详情里撤销", "ok");
      bumpData();
      void refreshStatus();
      onClose();
    } catch (e) {
      toast((e as Error).message, "err");
      setPhase("ready");
    }
  };

  const busy = phase === "loading" || phase === "applying";
  return (
    <Shell
      title="AI 整理"
      sub={model.label ? `模型:${model.label}` : undefined}
      onClose={onClose}
      busy={phase === "applying"}
      footer={phase === "ready" || phase === "applying" ? (
        <>
          <span className="mr-auto text-[11px] text-[var(--fg-faint)]">确认后覆盖原文并重新索引;旧版本会保存,可随时撤销</span>
          <button className="btn btn-ghost" disabled={busy} onClick={() => void start(true)}><RefreshIcon size={13} />重新生成</button>
          <button className="btn btn-secondary" disabled={busy} onClick={onClose}>取消</button>
          <button className="btn btn-primary" disabled={busy || !content.trim()} onClick={() => void apply()}>{phase === "applying" ? "覆盖中…" : "覆盖原文"}</button>
        </>
      ) : undefined}
    >
      {phase === "gate" && <CloudGate model={model.label} count={1} onGo={() => void go(model.ref)} />}
      {phase === "loading" && <Loading text="正在整理…" />}
      {phase === "error" && <ErrorBox text={error} onRetry={() => void start(true)} />}
      {data && (phase === "ready" || phase === "applying") && (
        <div className="space-y-3">
          <Notices warnings={data.warnings} notes={data.notes} />
          <div className="grid grid-cols-2 gap-4">
            <section>
              <div className="mb-1.5 text-[11px] font-medium text-[var(--fg-dim)]">原文{data.originalTitle ? ` · ${data.originalTitle}` : ""}</div>
              <div className="selectable max-h-[420px] min-h-[200px] overflow-y-auto whitespace-pre-wrap rounded-lg border border-[var(--line)] p-2.5 text-[13px] leading-relaxed text-[var(--fg-dim)]">{data.originalContent}</div>
            </section>
            <section>
              <div className="mb-1.5 text-[11px] font-medium text-[var(--accent)]">整理后(可直接修改)</div>
              <input className={`${fieldCls} mb-2 py-1.5 font-medium`} placeholder="标题" value={title} onChange={(e) => setTitle(e.target.value)} />
              <textarea className={`${fieldCls} h-[372px] resize-none`} value={content} onChange={(e) => setContent(e.target.value)} />
            </section>
          </div>
        </div>
      )}
    </Shell>
  );
}

/** 来源列表(编号即正文里的 [n]) */
function SourceList({ sources }: { sources: NoteBrief[] }) {
  return (
    <ol className="space-y-2">
      {sources.map((s, i) => (
        <li key={s.id} className="rounded-lg border border-[var(--line)] p-2.5 text-xs">
          <div className="mb-0.5 flex items-center gap-1.5 text-[var(--fg-dim)]">
            <span className="inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-[var(--accent-soft)] px-1 text-[11px] font-semibold text-[var(--accent)]">{i + 1}</span>
            <span>{fmtDate(s.createdAt)} {fmtTime(s.createdAt)}</span>
          </div>
          <div className="selectable line-clamp-4 whitespace-pre-wrap leading-relaxed">{s.title ? <b>{s.title} </b> : null}{s.preview}</div>
        </li>
      ))}
    </ol>
  );
}

/** 归并预览:左来源,右主题笔记(可编辑),确认后新建 */
function MergeView({ ids, onClose, onBack }: { ids: number[]; onClose: () => void; onBack?: () => void }) {
  const { toast, bumpData, refreshStatus, openDrawer } = useApp();
  const run = useCallback((ref: string | null) => api.organizeMergePropose(ids, ref), [ids]);
  const { phase, setPhase, data, error, model, start, go } = useProposal<MergeProposal>(run);
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  useEffect(() => { if (data) { setTitle(data.title); setContent(data.content); } }, [data]);

  const apply = async () => {
    if (!data) return;
    setPhase("applying");
    try {
      const id = await api.organizeMergeApply(data.ids, title, content);
      toast(`已创建主题笔记,${data.ids.length} 条原记录保留不变`, "ok");
      bumpData();
      void refreshStatus();
      if (onBack) onBack(); else onClose();
      openDrawer(id);
    } catch (e) {
      toast((e as Error).message, "err");
      setPhase("ready");
    }
  };

  const busy = phase === "loading" || phase === "applying";
  return (
    <Shell
      title="AI 归并"
      sub={`${ids.length} 条碎片${model.label ? ` · 模型:${model.label}` : ""}`}
      onClose={onClose}
      busy={phase === "applying"}
      footer={phase === "ready" || phase === "applying" ? (
        <>
          <span className="mr-auto text-[11px] text-[var(--fg-faint)]">会新建一条主题笔记并记录来源,原记录全部保留</span>
          {onBack && <button className="btn btn-ghost" disabled={busy} onClick={onBack}>返回列表</button>}
          <button className="btn btn-ghost" disabled={busy} onClick={() => void start(true)}><RefreshIcon size={13} />重新生成</button>
          <button className="btn btn-secondary" disabled={busy} onClick={onClose}>取消</button>
          <button className="btn btn-primary" disabled={busy || !content.trim()} onClick={() => void apply()}>{phase === "applying" ? "创建中…" : "创建主题笔记"}</button>
        </>
      ) : undefined}
    >
      {phase === "gate" && <CloudGate model={model.label} count={ids.length} onGo={() => void go(model.ref)} />}
      {phase === "loading" && <Loading text={`正在归并 ${ids.length} 条笔记…`} />}
      {phase === "error" && <ErrorBox text={error} onRetry={() => void start(true)} />}
      {data && (phase === "ready" || phase === "applying") && (
        <div className="space-y-3">
          <Notices warnings={data.warnings} notes={data.notes} />
          <div className="grid grid-cols-[320px_1fr] gap-4">
            <section>
              <div className="mb-1.5 text-[11px] font-medium text-[var(--fg-dim)]">来源(编号对应正文里的 [n])</div>
              <div className="max-h-[430px] overflow-y-auto pr-1"><SourceList sources={data.sources} /></div>
            </section>
            <section>
              <div className="mb-1.5 text-[11px] font-medium text-[var(--accent)]">主题笔记(可直接修改)</div>
              <input className={`${fieldCls} mb-2 py-1.5 font-medium`} placeholder="标题" value={title} onChange={(e) => setTitle(e.target.value)} />
              <textarea className={`${fieldCls} h-[372px] resize-none`} value={content} onChange={(e) => setContent(e.target.value)} />
            </section>
          </div>
        </div>
      )}
    </Shell>
  );
}

/** 碎片建议列表:按向量相似度聚好的组,点击后进入归并预览 */
function SuggestView({ onClose, onPick }: { onClose: () => void; onPick: (ids: number[]) => void }) {
  const kbId = useKb((s) => s.timelineKb);
  const [groups, setGroups] = useState<GroupSuggestion[] | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    setGroups(null);
    setError("");
    try { setGroups(await api.organizeSuggest(kbId)); } catch (e) { setError((e as Error).message); }
  }, [kbId]);
  useEffect(() => { void load(); }, [load]);

  return (
    <Shell title="整理碎片" sub="按语义相似度找出 30 天内可以合并的零散记录" onClose={onClose} busy={false}
      footer={<button className="btn btn-ghost" onClick={() => void load()}><RefreshIcon size={13} />重新查找</button>}
    >
      {error && <ErrorBox text={error} onRetry={() => void load()} />}
      {!error && groups === null && <Loading text="正在查找相近的碎片…" />}
      {groups && groups.length === 0 && (
        <div className="pt-20 text-center text-sm text-[var(--fg-dim)]">
          没有发现可归并的碎片
          <div className="mt-1 text-xs text-[var(--fg-faint)]">需要同一知识库、30 天内、主题相近的 3 条以上笔记。也可以在时间流里手动选择笔记再点「AI 归并」。</div>
        </div>
      )}
      {groups && groups.length > 0 && (
        <div className="space-y-3">
          {groups.map((g) => (
            <div key={g.ids.join("-")} className="rounded-xl border border-[var(--line)] bg-[var(--bg-elev)] p-3.5">
              <div className="mb-2 flex items-center gap-2">
                <span className="text-[13px] font-semibold">{g.items.length} 条相近碎片</span>
                <span className="text-[11px] text-[var(--fg-faint)]">平均相似度 {pct(g.avgSimilarity)}</span>
                <div className="flex-1" />
                <button className="btn btn-primary btn-sm" onClick={() => onPick(g.ids)}><SparkleIcon size={12} />预览归并</button>
              </div>
              <ul className="space-y-1">
                {g.items.map((it) => (
                  <li key={it.id} className="flex gap-2 text-xs">
                    <span className="w-28 shrink-0 text-[var(--fg-faint)]">{fmtDate(it.createdAt)} {fmtTime(it.createdAt)}</span>
                    <span className="line-clamp-1 text-[var(--fg-dim)]">{briefName(it, 60)}</span>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </Shell>
  );
}

/** 对话框入口,挂在 App 根部 */
export function OrganizeDialog() {
  const dialog = useOrganize((s) => s.dialog);
  const close = useOrganize((s) => s.close);
  const openSuggest = useOrganize((s) => s.openSuggest);
  // 从建议列表进入归并时,归并结果可以"返回列表"
  const [fromSuggest, setFromSuggest] = useState<number[] | null>(null);
  useEffect(() => { if (!dialog) setFromSuggest(null); }, [dialog]);

  if (!dialog) return null;
  if (dialog.kind === "single") return <SingleView key={`s${dialog.itemId}`} itemId={dialog.itemId} onClose={close} />;
  if (fromSuggest) {
    return <MergeView key={`m${fromSuggest.join("-")}`} ids={fromSuggest} onClose={close} onBack={() => { setFromSuggest(null); openSuggest(); }} />;
  }
  if (dialog.kind === "merge") return <MergeView key={`m${dialog.ids.join("-")}`} ids={dialog.ids} onClose={close} />;
  return <SuggestView onClose={close} onPick={(ids) => setFromSuggest(ids)} />;
}
