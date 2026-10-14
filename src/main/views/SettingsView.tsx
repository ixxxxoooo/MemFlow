// @author ygw
// 设置页:模型、检索、快捷键、数据与备份

import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "../../lib/api";
import { confirmDialog } from "../../lib/confirm";
import { eventToAccelerator, prettyAccelerator } from "../../lib/shortcut";
import { useApp } from "../../lib/store";
import type { Settings } from "../../lib/types";

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-6 border-b border-[var(--line)] py-3.5 last:border-0">
      <div className="max-w-[300px]">
        <div className="text-[13.5px] font-medium">{label}</div>
        {hint && <div className="mt-0.5 text-xs leading-snug text-[var(--fg-dim)]">{hint}</div>}
      </div>
      <div className="flex shrink-0 items-center">{children}</div>
    </div>
  );
}

const inputCls = "w-60 rounded-lg border border-[var(--line)] bg-[var(--bg-soft)] px-2.5 py-1.5 text-[13px] outline-none focus:border-[var(--accent)]";

export function SettingsView() {
  const { status, refreshStatus, toast, bumpData } = useApp();
  const [form, setForm] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [recording, setRecording] = useState(false);

  useEffect(() => { if (status && !form) setForm(status.settings); }, [status, form]);
  if (!form || !status) return <div className="p-8 text-sm text-[var(--fg-dim)]">加载中…</div>;

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setForm({ ...form, [k]: v });
  const dirty = JSON.stringify(form) !== JSON.stringify(status.settings);
  const embedChanged = form.embedModel !== status.settings.embedModel;

  const save = async () => {
    setSaving(true);
    try {
      await api.updateSettings(form);
      toast("设置已保存", "ok");
      await refreshStatus();
      if (embedChanged) toast("向量模型已更换,请在「知识档案库」执行全库重刷", "info");
    } catch (e) {
      toast((e as Error).message, "err");
    } finally {
      setSaving(false);
    }
  };

  const backup = async () => {
    const dir = await open({ directory: true, multiple: false, title: "选择备份保存位置" });
    if (!dir || Array.isArray(dir)) return;
    try {
      const out = await api.exportBackup(dir);
      toast(`备份完成:${out}`, "ok");
    } catch (e) {
      toast(`备份失败:${(e as Error).message}`, "err");
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div data-tauri-drag-region className="flex h-[52px] shrink-0 items-center px-8">
        <h2 className="text-[15px] font-semibold" data-tauri-drag-region>设置</h2>
        <div className="flex-1" data-tauri-drag-region />
        <button className="rounded-lg bg-[var(--accent)] px-4 py-1.5 text-xs font-medium text-white disabled:opacity-40" disabled={!dirty || saving} onClick={() => void save()}>
          {saving ? "保存中…" : "保存设置"}
        </button>
      </div>
      <div className="flex-1 overflow-y-auto px-8 pb-10">
        <div className="mx-auto max-w-[720px] space-y-7">
          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">Ollama 与模型</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="服务地址" hint="本机 Ollama 的 REST 地址,所有数据只在本机处理。"><input className={inputCls} value={form.ollamaUrl} onChange={(e) => set("ollamaUrl", e.target.value)} /></Row>
              <Row label="向量模型" hint={embedChanged ? "更换后旧向量不再参与检索,需全库重刷。" : "负责把文字转为向量,如 bge-m3、nomic-embed-text。"}><input className={inputCls} value={form.embedModel} onChange={(e) => set("embedModel", e.target.value)} /></Row>
              <Row label="文本模型" hint="负责问答、摘要、打标签、流程图,如 qwen2.5:7b。"><input className={inputCls} value={form.textModel} onChange={(e) => set("textModel", e.target.value)} /></Row>
              <Row label="启用视觉解析" hint="图片自动 OCR + 描述 + 标签。需要先下载视觉模型,开启后可对旧图片重刷补齐。">
                <input type="checkbox" className="h-4 w-4" checked={form.visionEnabled} onChange={(e) => set("visionEnabled", e.target.checked)} />
              </Row>
              <Row label="视觉模型" hint="推荐 minicpm-v(中文 OCR 强)或 qwen2.5vl。"><input className={inputCls} value={form.visionModel} onChange={(e) => set("visionModel", e.target.value)} /></Row>
              <Row label="模型常驻时长" hint="空闲多久后从内存卸载,如 5m、30s、1h、-1(常驻)。"><input className={`${inputCls} w-28`} value={form.keepAlive} onChange={(e) => set("keepAlive", e.target.value)} /></Row>
              <Row label="上下文长度" hint="所有请求保持一致,避免 Ollama 反复重载模型。内存紧张可调小。"><input type="number" className={`${inputCls} w-28`} value={form.numCtx} onChange={(e) => set("numCtx", Number(e.target.value))} /></Row>
            </div>
            <div className="mt-2 flex flex-wrap gap-2 text-xs text-[var(--fg-dim)]">
              {status.models.map((m) => (
                <span key={m.name} className="rounded-full border border-[var(--line)] px-2.5 py-1">
                  {m.role} {m.name}:{m.installed ? <span className="text-[var(--ok)]">已安装</span> : <span className={m.required ? "text-[var(--err)]" : ""}>{m.required ? "未安装" : "未安装(未启用)"}</span>}
                </span>
              ))}
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">检索与问答</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="相关性阈值" hint="越高越严格:低于阈值的内容不会作为资料,没有资料时 AI 会直接说明找不到。">
                <div className="flex items-center gap-3"><input type="range" min={0.1} max={0.8} step={0.01} value={form.scoreThreshold} onChange={(e) => set("scoreThreshold", Number(e.target.value))} /><span className="w-10 text-right text-xs">{form.scoreThreshold.toFixed(2)}</span></div>
              </Row>
              <Row label="引用资料条数" hint="每次问答最多带入多少条资料。"><input type="number" min={1} max={30} className={`${inputCls} w-24`} value={form.topK} onChange={(e) => set("topK", Number(e.target.value))} /></Row>
              <Row label="追问时改写问题" hint="多轮对话中先把带指代的追问改写成完整问句再检索(多一次模型调用)。"><input type="checkbox" className="h-4 w-4" checked={form.queryRewrite} onChange={(e) => set("queryRewrite", e.target.checked)} /></Row>
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">快捷键</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="唤起闪念胶囊" hint="点击右侧后按下新的组合键(需包含 ⌘ / ⌥ / ⇧ 之一)。">
                <button
                  className={`min-w-[150px] rounded-lg border px-3 py-1.5 text-[13px] ${recording ? "border-[var(--accent)] text-[var(--accent)]" : "border-[var(--line)] bg-[var(--bg-soft)]"}`}
                  onClick={() => setRecording(true)}
                  onBlur={() => setRecording(false)}
                  onKeyDown={(e) => {
                    if (!recording) return;
                    e.preventDefault();
                    if (e.key === "Escape") { setRecording(false); return; }
                    const acc = eventToAccelerator(e.nativeEvent);
                    if (acc) { set("shortcut", acc); setRecording(false); }
                  }}
                >
                  {recording ? "请按下快捷键…" : prettyAccelerator(form.shortcut)}
                </button>
              </Row>
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">数据</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="数据位置" hint="单个 SQLite 文件 + 附件目录,拷走即可迁移。"><span className="selectable max-w-[260px] break-all text-xs text-[var(--fg-dim)]">{status.dataDir}</span></Row>
              <Row label="统计" hint={`向量索引已载入 ${status.vectorCount} 条`}><span className="text-xs text-[var(--fg-dim)]">{status.stats.notes} 记录 · {status.stats.images} 图片 · {status.stats.documents} 文档 · {status.stats.chunks} 切片</span></Row>
              <Row label="备份数据" hint="导出数据库一致性快照与全部附件到所选文件夹。"><button className="rounded-lg border border-[var(--line)] px-3 py-1.5 text-xs hover:bg-[var(--bg-soft)]" onClick={() => void backup()}>选择位置并备份</button></Row>
              <Row label="全库重刷" hint="用当前模型与提示词重新解析所有内容,旧索引在新结果就绪后才会被替换。">
                <button className="rounded-lg border border-[var(--line)] px-3 py-1.5 text-xs hover:bg-[var(--bg-soft)]" onClick={async () => {
                  if (!(await confirmDialog("确定对全部内容执行重刷吗?"))) return;
                  try { const n = await api.reindex(null); toast(`已加入 ${n} 项到重刷队列`, "ok"); bumpData(); void refreshStatus(); } catch (e) { toast((e as Error).message, "err"); }
                }}>全库重刷</button>
              </Row>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
