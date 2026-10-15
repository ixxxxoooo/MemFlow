// @author ygw
// 向量/视觉模型的来源选择:本地(Ollama)或云端(复用已配置的云端档案,再指定模型名)。
// 本地时渲染调用方传入的本地模型输入行;云端时显示档案、模型名与连接测试。

import { useState, type ReactNode } from "react";
import { api } from "../../lib/api";
import { confirmDialog } from "../../lib/confirm";
import { useApp } from "../../lib/store";
import type { ModelProvider, Settings } from "../../lib/types";
import { ConnResult, type ConnState } from "./ConnResult";
import { inputCls, Row } from "./SettingsRow";

type Kind = "embed" | "vision";

const TEXT = {
  embed: {
    title: "向量模型",
    cloudHint: "把文字转为向量。需要服务商提供 /embeddings 接口(如通义 text-embedding-v3、硅基流动 BAAI/bge-m3、OpenAI text-embedding-3-small);DeepSeek 官方没有。",
    placeholder: "如 text-embedding-v3",
    privacy: "选择云端向量模型后,你的笔记与文档文本片段(导入、重建索引、提问时)会发送给所选服务商用于生成向量。\n\n原始数据与向量索引仍只保存在本机。确定使用云端向量模型吗?",
  },
  vision: {
    title: "视觉模型",
    cloudHint: "解析图片与 PDF 页面。需要支持图片输入的多模态模型(如通义 qwen-vl-plus、Qwen2.5-VL、gpt-4o-mini)。",
    placeholder: "如 qwen-vl-plus",
    privacy: "选择云端视觉模型后,启用视觉解析的图片与 PDF 页面图像会发送给所选服务商。\n\n确定使用云端视觉模型吗?",
  },
} as const;

interface Props {
  kind: Kind;
  form: Settings;
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void;
  /** 已保存密钥的档案标识 */
  keyedProfiles: string[];
  /** 默认云端配置输入框里尚未保存的密钥(测试连接时优先使用) */
  pendingDefaultKey: string;
  /** 本地模式下显示的模型输入行 */
  children: ReactNode;
}

export function ModelSourceRows({ kind, form, set, keyedProfiles, pendingDefaultKey, children }: Props) {
  const toast = useApp((s) => s.toast);
  const t = TEXT[kind];
  const providerKey = `${kind}Provider` as const;
  const profileKey = `${kind}Profile` as const;
  const modelKey = `${kind}CloudModel` as const;
  const provider: ModelProvider = form[providerKey];
  const profile = form[profileKey];
  const model = form[modelKey];
  const [conn, setConn] = useState<ConnState | null>(null);
  const [testing, setTesting] = useState(false);

  const keyed = keyedProfiles.includes(profile) || (profile === "default" && pendingDefaultKey.trim() !== "");

  const chooseProvider = async (p: ModelProvider) => {
    if (p === provider) return;
    if (p === "openai_compat" && !(await confirmDialog(t.privacy))) return;
    setConn(null);
    set(providerKey, p);
  };

  const test = async () => {
    setTesting(true);
    setConn(null);
    try {
      const msg = await api.testCloudModel(kind, profile === "default" ? null : profile, model, profile === "default" ? pendingDefaultKey.trim() || null : null);
      setConn({ ok: true, msg });
      toast(msg, "ok");
    } catch (e) {
      setConn({ ok: false, msg: (e as Error).message });
      toast((e as Error).message, "err");
    } finally {
      setTesting(false);
    }
  };

  return (
    <>
      <Row label={`${t.title}来源`} hint="可以选本机 Ollama 或云端服务。选云端可免下载本地模型。">
        <select
          aria-label={`${t.title}来源`}
          className={`${inputCls} w-60`}
          value={provider}
          onChange={(e) => void chooseProvider(e.target.value as ModelProvider)}
        >
          <option value="ollama">本地(Ollama)</option>
          <option value="openai_compat">云端(OpenAI 兼容)</option>
        </select>
      </Row>
      {provider === "ollama" ? (
        children
      ) : (
        <>
          <Row label="云端档案" hint="复用上方已配置的云端模型地址与 API Key。">
            <select
              aria-label={`${t.title}云端档案`}
              className={`${inputCls} w-60`}
              value={profile}
              onChange={(e) => { setConn(null); set(profileKey, e.target.value); }}
            >
              <option value="default">默认云端配置{keyedProfiles.includes("default") ? "" : "(未保存密钥)"}</option>
              {form.llmProfiles.map((p) => (
                <option key={p.id} value={p.id}>{p.name}{keyedProfiles.includes(p.id) ? "" : "(未保存密钥)"}</option>
              ))}
            </select>
          </Row>
          <Row
            label={`云端${t.title}`}
            hint={<>{t.cloudHint}{!keyed && <span className="text-[var(--err)]"> 所选档案还没有 API Key,请先在上方填写并保存。</span>}</>}
          >
            <div className="flex items-center gap-2">
              <input
                aria-label={`云端${t.title}名`}
                className={`${inputCls} w-52`}
                value={model}
                placeholder={t.placeholder}
                onChange={(e) => { setConn(null); set(modelKey, e.target.value); }}
              />
              <button className="btn btn-secondary btn-sm shrink-0" disabled={testing || !model.trim()} onClick={() => void test()}>
                {testing ? "测试中…" : "测试"}
              </button>
              <ConnResult state={conn} />
            </div>
          </Row>
        </>
      )}
    </>
  );
}
