// @author ygw
// 额外云端模型档案编辑器:每个档案有独立的地址、模型与钥匙串密钥,对话里可随时切换

import { useState } from "react";
import { api } from "../../lib/api";
import { confirmDialog } from "../../lib/confirm";
import { findPreset, LLM_PRESETS } from "../../lib/llmPresets";
import { useApp } from "../../lib/store";
import type { LlmProfile } from "../../lib/types";
import { BrandIcon } from "./BrandIcon";
import { PlusIcon, TrashIcon } from "./icons";

const inputCls = "w-full rounded-lg border border-[var(--line)] bg-[var(--bg-soft)] px-2.5 py-1.5 text-[13px] outline-none focus:border-[var(--accent)]";

/** 生成稳定的档案标识:字母数字,不与 default 冲突 */
function newId(): string {
  return `p${Math.random().toString(36).slice(2, 8)}`;
}

interface Props {
  profiles: LlmProfile[];
  onChange: (next: LlmProfile[]) => void;
}

export function ProfilesEditor({ profiles, onChange }: Props) {
  const { status, toast, refreshStatus } = useApp();
  // 每个档案的密钥输入框:只在内存中短暂停留
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [busyId, setBusyId] = useState<string | null>(null);
  const keyed = new Set(status?.keyedProfiles ?? []);

  const patch = (id: string, p: Partial<LlmProfile>) =>
    onChange(profiles.map((x) => (x.id === id ? { ...x, ...p } : x)));

  const add = () => {
    const preset = findPreset("deepseek")!;
    onChange([...profiles, { id: newId(), name: preset.label, preset: preset.id, baseUrl: preset.baseUrl, model: preset.model }]);
  };

  const choosePreset = (p: LlmProfile, presetId: string) => {
    const pr = findPreset(presetId);
    patch(p.id, {
      preset: presetId,
      name: presetId === "custom" ? p.name : (pr?.label ?? p.name),
      baseUrl: pr?.baseUrl ? pr.baseUrl : p.baseUrl,
      model: pr?.model ? pr.model : p.model,
    });
  };

  const saveKey = async (p: LlmProfile) => {
    const k = (keys[p.id] ?? "").trim();
    if (!k) return;
    setBusyId(p.id);
    try {
      await api.setLlmApiKey(k, p.id);
      setKeys((m) => ({ ...m, [p.id]: "" }));
      toast("密钥已保存到钥匙串", "ok");
      await refreshStatus();
    } catch (e) {
      toast((e as Error).message, "err");
    } finally {
      setBusyId(null);
    }
  };

  const clearKey = async (p: LlmProfile) => {
    if (!(await confirmDialog(`确定删除「${p.name}」已保存的 API Key 吗?`))) return;
    try {
      await api.setLlmApiKey("", p.id);
      await refreshStatus();
    } catch (e) {
      toast((e as Error).message, "err");
    }
  };

  const test = async (p: LlmProfile) => {
    setBusyId(p.id);
    try {
      const msg = await api.testLlmConnection(p.baseUrl, p.model, (keys[p.id] ?? "").trim() || null, p.id);
      toast(msg, "ok");
    } catch (e) {
      toast((e as Error).message, "err");
    } finally {
      setBusyId(null);
    }
  };

  const remove = async (p: LlmProfile) => {
    if (!(await confirmDialog(`删除模型档案「${p.name}」?已保存的密钥也会一并清除。`))) return;
    try {
      if (keyed.has(p.id)) await api.setLlmApiKey("", p.id);
    } catch {
      /* 密钥清除失败不阻止档案删除 */
    }
    onChange(profiles.filter((x) => x.id !== p.id));
    await refreshStatus();
  };

  return (
    <div className="space-y-3 py-3.5">
      {profiles.length === 0 && (
        <div className="text-xs leading-relaxed text-[var(--fg-dim)]">
          可添加多个云端服务商,在问答输入框旁一键切换回答所用的模型。每个档案的密钥单独保存在 macOS 钥匙串。
        </div>
      )}
      {profiles.map((p) => {
        const has = keyed.has(p.id);
        const busy = busyId === p.id;
        return (
          <div key={p.id} className="rounded-xl border border-[var(--line)] p-3.5">
            <div className="mb-2.5 flex items-center gap-2">
              <BrandIcon model={p.model} preset={p.preset} provider="openai_compat" size={18} />
              <input className={`${inputCls} flex-1 font-medium`} value={p.name} onChange={(e) => patch(p.id, { name: e.target.value })} placeholder="显示名称" />
              <select className="select w-44" value={p.preset} onChange={(e) => choosePreset(p, e.target.value)}>
                {LLM_PRESETS.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
              </select>
              <button className="btn btn-ghost btn-sm" title="删除档案" onClick={() => void remove(p)}><TrashIcon size={14} /></button>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <input className={inputCls} value={p.baseUrl} onChange={(e) => patch(p.id, { baseUrl: e.target.value, preset: "custom" })} placeholder="接口地址 https://api.deepseek.com" />
              <input className={inputCls} value={p.model} onChange={(e) => patch(p.id, { model: e.target.value })} placeholder="模型名 deepseek-chat" />
            </div>
            <div className="mt-2 flex items-center gap-2">
              <input
                type="password"
                autoComplete="off"
                className={inputCls}
                value={keys[p.id] ?? ""}
                onChange={(e) => setKeys((m) => ({ ...m, [p.id]: e.target.value }))}
                placeholder={has ? "●●●●●●●●(已保存,填写新值可替换)" : "API Key,sk-..."}
              />
              <button className="btn btn-secondary btn-sm shrink-0" disabled={busy || !(keys[p.id] ?? "").trim()} onClick={() => void saveKey(p)}>保存密钥</button>
              {has && <button className="btn btn-ghost btn-sm shrink-0" onClick={() => void clearKey(p)}>清除</button>}
              <button className="btn btn-secondary btn-sm shrink-0" disabled={busy || !p.baseUrl || !p.model || (!has && !(keys[p.id] ?? "").trim())} onClick={() => void test(p)}>{busy ? "…" : "测试"}</button>
            </div>
            <div className="mt-1.5 text-[11px] text-[var(--fg-faint)]">
              {has ? "密钥已就绪,对话里可选用此模型。" : "保存密钥后,对话里才可选用此模型。"}修改地址/模型后需点右上角「保存设置」。
            </div>
          </div>
        );
      })}
      <button className="btn btn-secondary btn-sm" disabled={profiles.length >= 12} onClick={add}><PlusIcon size={13} />添加云端模型</button>
    </div>
  );
}
