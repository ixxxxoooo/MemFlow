// @author ygw
// 设置页:模型、检索、快捷键、数据与备份

import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import { confirmDialog } from "../../lib/confirm";
import { findPreset, LLM_PRESETS } from "../../lib/llmPresets";
import { eventToAccelerator, prettyAccelerator } from "../../lib/shortcut";
import { useApp } from "../../lib/store";
import type { Settings } from "../../lib/types";
import { ConnResult, type ConnState } from "../components/ConnResult";
import { SecretInput } from "../components/SecretInput";
import { APP_SHORTCUTS } from "../../lib/appShortcuts";
import { ProfilesEditor } from "../components/ProfilesEditor";
import { ModelSourceRows } from "../components/ModelSourceRows";
import { inputCls, Row } from "../components/SettingsRow";
import { embedIdentity } from "../../lib/modelSource";
import { PullControls } from "../components/PullControls";
import { RerankControls } from "../components/RerankControls";
import type { RerankModel } from "../../lib/types";

export function SettingsView() {
  const { status, refreshStatus, toast, bumpData } = useApp();
  const [form, setForm] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [recording, setRecording] = useState(false);
  // 云端 API Key 输入框内容:只在内存中短暂停留,保存后立即清空
  const [keyInput, setKeyInput] = useState("");
  const [testing, setTesting] = useState(false);
  // 连接测试结果:成功显示绿色对号;地址/模型/密钥任何一项改动后清除,避免展示过期结论
  const [conn, setConn] = useState<ConnState | null>(null);
  // 本地重排序模型清单;下载结束(pull 回到 null)后刷新安装状态
  const [rerankModels, setRerankModels] = useState<RerankModel[]>([]);
  const pulling = useApp((s) => s.pull);
  useEffect(() => { if (!pulling) void api.rerankModels().then(setRerankModels).catch(() => {}); }, [pulling]);

  useEffect(() => { if (status && !form) setForm(status.settings); }, [status, form]);
  if (!form || !status) return <div className="p-8 text-sm text-[var(--fg-dim)]">加载中…</div>;

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => { setConn(null); setForm({ ...form, [k]: v }); };
  const dirty = JSON.stringify(form) !== JSON.stringify(status.settings) || keyInput.trim() !== "";
  const settingsDirty = JSON.stringify(form) !== JSON.stringify(status.settings);
  const cloud = form.llmProvider === "openai_compat";
  const preset = findPreset(form.llmPreset);
  const embedChanged = embedIdentity(form) !== embedIdentity(status.settings);

  const save = async () => {
    setSaving(true);
    try {
      if (keyInput.trim()) {
        await api.setLlmApiKey(keyInput.trim());
        setKeyInput("");
      }
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

  // 切换到云端前提示隐私影响
  const chooseProvider = async (p: Settings["llmProvider"]) => {
    if (p === form.llmProvider) return;
    if (p === "openai_compat") {
      const ok = await confirmDialog(
        "切换到云端模型后,你的提问内容和检索到的资料片段会发送给第三方服务商(如 DeepSeek)用于生成回答。\n\n向量索引与原始数据仍只保存在本机。确定要使用云端模型吗?",
      );
      if (!ok) return;
    }
    set("llmProvider", p);
  };

  const choosePreset = (id: string) => {
    setConn(null);
    const p = findPreset(id);
    setForm({
      ...form,
      llmPreset: id,
      llmBaseUrl: p && p.baseUrl ? p.baseUrl : form.llmBaseUrl,
      llmModel: p && p.model ? p.model : form.llmModel,
    });
  };

  const testConn = async () => {
    setTesting(true);
    setConn(null);
    try {
      const msg = await api.testLlmConnection(form.llmBaseUrl, form.llmModel, keyInput.trim() || null);
      setConn({ ok: true, msg });
      toast(msg, "ok");
    } catch (e) {
      setConn({ ok: false, msg: (e as Error).message });
      toast((e as Error).message, "err");
    } finally {
      void refreshStatus();
      setTesting(false);
    }
  };

  // 钥匙串读取失败时的原因(默认档案)
  const keyErr = status.keyErrors?.find((k) => k.profile === "default")?.message ?? null;

  const retryKey = async () => {
    try {
      await api.retryKeyAccess();
      toast("已读取到 API Key", "ok");
    } catch (e) {
      toast((e as Error).message, "err");
    }
    await refreshStatus();
  };

  const clearKey = async () => {
    if (!(await confirmDialog("确定从钥匙串中删除已保存的 API Key 吗?"))) return;
    try {
      await api.setLlmApiKey("");
      setKeyInput("");
      toast("已清除 API Key", "ok");
      await refreshStatus();
    } catch (e) {
      toast((e as Error).message, "err");
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
        <button className="btn btn-primary px-4" disabled={!dirty || saving} onClick={() => void save()}>
          {saving ? "保存中…" : "保存设置"}
        </button>
      </div>
      <div className="flex-1 overflow-y-auto px-8 pb-10">
        <div className="mx-auto max-w-[720px] space-y-7">
          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">文本模型来源</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="问答 / 打标签 / 流程图使用" hint="本地 Ollama 完全离线;云端 API 响应更快、效果更强,但提问与资料片段会发给服务商。向量化始终在本机完成。">
                <div className="seg" role="group" aria-label="文本模型来源">
                  <button aria-pressed={!cloud} onClick={() => void chooseProvider("ollama")}>本地 Ollama</button>
                  <button aria-pressed={cloud} onClick={() => void chooseProvider("openai_compat")}>云端 API</button>
                </div>
              </Row>
              {cloud ? (
                <>
                  <Row label="服务商" hint={preset?.keyUrl ? `预设会自动填入地址与默认模型。密钥申请:${preset.keyUrl}` : "任何兼容 OpenAI /chat/completions 的服务均可,选「自定义」手动填写。"}>
                    <select className="select w-60" value={form.llmPreset} onChange={(e) => choosePreset(e.target.value)}>
                      {LLM_PRESETS.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
                    </select>
                  </Row>
                  <Row label="接口地址" hint="含版本路径,实际请求 {地址}/chat/completions。"><input className={inputCls} value={form.llmBaseUrl} onChange={(e) => { setConn(null); setForm({ ...form, llmBaseUrl: e.target.value, llmPreset: "custom" }); }} placeholder="https://api.deepseek.com" /></Row>
                  <Row label="模型名" hint="DeepSeek 常用 deepseek-chat(通用)或 deepseek-reasoner(推理,更慢)。"><input className={inputCls} value={form.llmModel} onChange={(e) => set("llmModel", e.target.value)} placeholder="deepseek-chat" /></Row>
                  <Row label="API Key" hint={keyErr ? `已保存,但读取失败:${keyErr}` : status.hasApiKey ? "已保存在 macOS 钥匙串,不会写入数据库或备份。填写新值并保存即可替换。" : "保存在 macOS 钥匙串,不会写入数据库或备份。"}>
                    <div className="flex items-center gap-2">
                      <SecretInput
                        className={inputCls.replace("w-60", "w-full")}
                        wrapperClass="w-60"
                        value={keyInput}
                        onChange={(v) => { setConn(null); setKeyInput(v); }}
                        saved={status.hasApiKey}
                        placeholder={status.hasApiKey ? "●●●●●●●●(已保存)" : "sk-..."}
                      />
                      {keyErr && <button className="btn btn-secondary btn-sm" onClick={() => void retryKey()}>重新授权</button>}
                      {status.hasApiKey && <button className="btn btn-ghost btn-sm" onClick={() => void clearKey()}>清除</button>}
                    </div>
                  </Row>
                  <Row label="连接测试" hint="使用上面填写的地址、模型与密钥发送一个极小请求(无需先保存)。">
                    <div className="flex items-center gap-3">
                      <ConnResult state={conn} />
                      <button className="btn btn-secondary" disabled={testing || !form.llmBaseUrl || !form.llmModel || (!keyInput.trim() && !status.hasApiKey)} onClick={() => void testConn()}>{testing ? "测试中…" : "测试连接"}</button>
                    </div>
                  </Row>
                  <div className="my-3 rounded-lg bg-[var(--accent-soft)] px-3 py-2 text-xs leading-relaxed text-[var(--fg-dim)]">
                    隐私提示:云端模式下,问题与检索到的资料片段会发送给所选服务商。若资料包含敏感信息,请使用本地 Ollama。
                  </div>
                </>
              ) : (
                <Row label="本地文本模型" hint="负责问答、摘要、打标签、流程图,如 qwen2.5:7b。下载见下方「本地模型」。"><input className={inputCls} value={form.textModel} onChange={(e) => set("textModel", e.target.value)} /></Row>
              )}
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">更多云端模型(问答中可切换)</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <ProfilesEditor profiles={form.llmProfiles} onChange={(next) => set("llmProfiles", next)} />
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">向量与视觉模型(本地或云端)</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <ModelSourceRows kind="embed" form={form} set={set} keyedProfiles={status.keyedProfiles} pendingDefaultKey={keyInput}>
                <Row label="向量模型" hint={embedChanged ? "更换后旧向量不再参与检索,需全库重刷。" : "负责把文字转为向量,如 bge-m3、nomic-embed-text。"}><input className={inputCls} value={form.embedModel} onChange={(e) => set("embedModel", e.target.value)} /></Row>
              </ModelSourceRows>
              <Row label="启用视觉解析" hint="图片自动 OCR + 描述 + 标签。需要先下载视觉模型,开启后可对旧图片重刷补齐。">
                <input type="checkbox" className="switch" checked={form.visionEnabled} onChange={(e) => set("visionEnabled", e.target.checked)} />
              </Row>
              <ModelSourceRows kind="vision" form={form} set={set} keyedProfiles={status.keyedProfiles} pendingDefaultKey={keyInput}>
                <Row label="视觉模型" hint="推荐 minicpm-v(中文 OCR 强)或 qwen2.5vl。"><input className={inputCls} value={form.visionModel} onChange={(e) => set("visionModel", e.target.value)} /></Row>
              </ModelSourceRows>
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">本地模型(Ollama)</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="服务地址" hint="本机 Ollama 的 REST 地址。选择本地来源的模型会在这里运行。"><input className={inputCls} value={form.ollamaUrl} onChange={(e) => set("ollamaUrl", e.target.value)} /></Row>
              <Row label="导入文档时自动 AI 解读" hint="默认关闭:文档导入只做分块与向量索引,速度快,检索不受影响。开启后会额外调用文本模型生成摘要/标签(较慢);也可在文档弹窗里按需单独解读。">
                <input type="checkbox" className="switch" checked={form.docAiEnrich} onChange={(e) => set("docAiEnrich", e.target.checked)} />
              </Row>
              <Row label="模型常驻时长" hint="空闲多久后从内存卸载,如 5m、30s、1h、-1(常驻)。"><input className={`${inputCls} w-28`} value={form.keepAlive} onChange={(e) => set("keepAlive", e.target.value)} /></Row>
              <Row label="上下文长度" hint="所有请求保持一致,避免 Ollama 反复重载模型。内存紧张可调小。"><input type="number" className={`${inputCls} w-28`} value={form.numCtx} onChange={(e) => set("numCtx", Number(e.target.value))} /></Row>
              <Row label="入库并发数" hint="同时处理的文档数。0 为自动(按 CPU 核数,最多 4)。向量化与本地大模型调用仍按各自上限限流,调大不会撑爆内存。"><input type="number" min={0} max={8} className={`${inputCls} w-28`} value={form.importConcurrency} onChange={(e) => set("importConcurrency", Math.max(0, Math.min(8, Number(e.target.value) || 0)))} /></Row>
              <Row label="PDF 解析模式" hint="自动:读取文本层,扫描页用本机 OCR,表格还原为 Markdown(快,推荐)。视觉模型:逐页交给视觉模型转 Markdown,版面/表格更准但很慢,需先下载视觉模型。">
                <select className={`${inputCls} w-40`} value={form.pdfParseMode} onChange={(e) => set("pdfParseMode", e.target.value as "auto" | "vision")}>
                  <option value="auto">自动(本机 OCR)</option>
                  <option value="vision">视觉模型(高精度)</option>
                </select>
              </Row>
              <Row label="本地重排序" hint="对检索召回的候选片段用本地模型再次精排,显著提升答案相关性;只在检索时多花约 1 秒。模型未下载时自动跳过。">
                <input type="checkbox" className="switch" checked={form.rerankEnabled} onChange={(e) => set("rerankEnabled", e.target.checked)} />
              </Row>
              {form.rerankEnabled && (
                <>
                  <Row label="重排序模型" hint="base 体积小速度快(推荐);v2-m3 多语言更准但更慢更大。">
                    <select className={`${inputCls} w-72`} value={form.rerankModel} onChange={(e) => set("rerankModel", e.target.value)}>
                      {rerankModels.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
                    </select>
                  </Row>
                  {(() => {
                    const m = rerankModels.find((x) => x.id === form.rerankModel);
                    return m ? (
                      <Row label="模型文件" hint="保存在应用数据目录的 models/rerank 下;先走镜像,失败自动回退官方站点,支持断点续传与 sha256 校验。">
                        <RerankControls id={m.id} installed={m.installed} size={m.size} />
                      </Row>
                    ) : null;
                  })()}
                  <Row label="下载镜像" hint="HuggingFace 镜像地址,国内网络可用 hf-mirror.com。"><input className={inputCls} value={form.hfMirror} onChange={(e) => set("hfMirror", e.target.value)} /></Row>
                </>
              )}
              <Row label="直连下载仓库" hint="「直连下载」使用的 Ollama 模型仓库地址,网络受限时可改成可访问的镜像。"><input className={inputCls} value={form.modelRegistry} onChange={(e) => set("modelRegistry", e.target.value)} /></Row>
            </div>

            <h3 className="mb-1 mt-5 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">模型状态与下载</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              {status.models.map((m) => (
                <div key={`${m.role}-${m.name}`} className="flex items-center justify-between gap-4 border-b border-[var(--line)] py-3 last:border-0">
                  <div className="min-w-0">
                    <div className="text-[13.5px] font-medium">{m.role} · <span className="selectable">{m.name}</span></div>
                    <div className="mt-0.5 text-xs">
                      {!m.local
                        ? (m.installed ? <span className="text-[var(--ok)]">云端模型 · 已配置密钥</span> : <span className="text-[var(--err)]">云端模型 · 未配置密钥</span>)
                        : m.installed
                          ? <span className="text-[var(--ok)]">已安装</span>
                          : <span className={m.required ? "text-[var(--err)]" : "text-[var(--fg-dim)]"}>{m.required ? "未安装" : "未安装(未启用)"}</span>}
                    </div>
                  </div>
                  {m.local && !m.installed && (
                    <PullControls name={m.name} disabled={settingsDirty} disabledReason="请先保存设置,再下载当前填写的模型" />
                  )}
                </div>
              ))}
              <div className="py-3 text-xs leading-relaxed text-[var(--fg-dim)]">
                下载失败并提示 redirect target not allowed 或地址为 198.18.x.x,是代理开启了 fake-ip 导致的,请使用「直连下载」。也可在终端运行 scripts/pull_model_manual.sh。
              </div>
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">检索与问答</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="相关性阈值" hint="越高越严格:低于阈值的内容不会作为资料,没有资料时 AI 会直接说明找不到。">
                <div className="flex items-center gap-3"><input type="range" min={0.1} max={0.8} step={0.01} value={form.scoreThreshold} onChange={(e) => set("scoreThreshold", Number(e.target.value))} /><span className="w-10 text-right text-xs">{form.scoreThreshold.toFixed(2)}</span></div>
              </Row>
              <Row label="引用资料条数" hint="每次问答最多带入多少条资料。"><input type="number" min={1} max={30} className={`${inputCls} w-24`} value={form.topK} onChange={(e) => set("topK", Number(e.target.value))} /></Row>
              <Row label="追问时改写问题" hint="多轮对话中先把带指代的追问改写成完整问句再检索(多一次模型调用)。"><input type="checkbox" className="switch" checked={form.queryRewrite} onChange={(e) => set("queryRewrite", e.target.checked)} /></Row>
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">快捷键</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="唤起闪念胶囊" hint="点击右侧后按下新的组合键(需包含 ⌘ / ⌥ / ⇧ 之一)。">
                <button
                  className={`btn min-w-[150px] text-[13px] ${recording ? "border-[var(--accent)] text-[var(--accent)]" : "btn-secondary bg-[var(--bg-soft)]"}`}
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
              <Row label="应用内快捷键" hint="在 MemFlow 窗口内可用。">
                <div className="space-y-1 text-[12.5px]" data-testid="app-shortcuts">
                  {APP_SHORTCUTS.map((sc) => (
                    <div key={sc.action} className="flex items-center justify-between gap-6">
                      <span className="text-[var(--fg-dim)]">{sc.label}</span>
                      <kbd className="rounded border border-[var(--line)] px-1.5 text-[11.5px]">{sc.keys}</kbd>
                    </div>
                  ))}
                </div>
              </Row>
            </div>
          </section>

          <section>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--fg-faint)]">数据</h3>
            <div className="rounded-2xl border border-[var(--line)] px-5">
              <Row label="数据位置" hint="单个 SQLite 文件 + 附件目录,拷走即可迁移。"><span className="selectable max-w-[260px] break-all text-xs text-[var(--fg-dim)]">{status.dataDir}</span></Row>
              <Row label="统计" hint={`向量索引已载入 ${status.vectorCount} 条`}><span className="text-xs text-[var(--fg-dim)]">{status.stats.notes} 记录 · {status.stats.images} 图片 · {status.stats.documents} 文档 · {status.stats.chunks} 切片</span></Row>
              <Row label="运行日志" hint="按天滚动、保留 7 天;反馈问题时请附上最近的日志文件。"><button className="btn btn-secondary" onClick={() => void api.openLogDir().catch((e: Error) => toast(`打开失败:${e.message}`, "err"))}>打开日志目录</button></Row>
              <Row label="备份数据" hint="导出数据库一致性快照与全部附件到所选文件夹。"><button className="btn btn-secondary" onClick={() => void backup()}>选择位置并备份</button></Row>
              <Row label="全库重刷" hint="用当前模型与提示词重新解析所有内容,旧索引在新结果就绪后才会被替换。">
                <button className="btn btn-secondary" onClick={async () => {
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
