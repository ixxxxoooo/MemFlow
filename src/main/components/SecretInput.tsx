// @author ixxxxoooo
// API Key 输入框:右侧眼睛图标切换明文。
// - 输入框里有内容:直接切换显示/隐藏。
// - 输入为空但钥匙串里已保存:点击后向后端取回明文显示(会触发钥匙串授权),10 秒或离开后自动隐藏,不缓存。

import { useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { useApp } from "../../lib/store";
import { EyeIcon, EyeOffIcon } from "./icons";

interface Props {
  value: string;
  onChange: (v: string) => void;
  /** 钥匙串里是否已保存 */
  saved: boolean;
  /** 档案标识;null 表示默认云端模型 */
  profileId?: string | null;
  placeholder?: string;
  /** 输入框样式(宽度由 wrapperClass 决定) */
  className?: string;
  /** 外层容器样式,默认撑满剩余宽度 */
  wrapperClass?: string;
}

const AUTO_HIDE_MS = 10_000;

export function SecretInput({ value, onChange, saved, profileId = null, placeholder, className = "", wrapperClass = "flex-1" }: Props) {
  const toast = useApp((s) => s.toast);
  const [shown, setShown] = useState(false);
  // 从钥匙串取回的明文(仅在展示期间存在)
  const [revealed, setRevealed] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const hide = () => {
    setShown(false);
    setRevealed(null);
    if (timer.current) clearTimeout(timer.current);
  };
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);

  const toggle = async () => {
    if (shown || revealed != null) { hide(); return; }
    if (value) { setShown(true); return; }
    if (!saved) return;
    setLoading(true);
    try {
      setRevealed(await api.revealApiKey(profileId));
      timer.current = setTimeout(hide, AUTO_HIDE_MS);
    } catch (e) {
      toast((e as Error).message, "err");
    } finally {
      setLoading(false);
    }
  };

  const showing = shown || revealed != null;
  const canToggle = !!value || saved;

  return (
    <div className={`relative ${wrapperClass}`} onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) hide(); }}>
      <input
        type={showing ? "text" : "password"}
        autoComplete="off"
        spellCheck={false}
        className={`${className} pr-8`}
        value={revealed ?? value}
        readOnly={revealed != null}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
      />
      <button
        type="button"
        aria-label={showing ? "隐藏 API Key" : "显示 API Key"}
        title={showing ? "隐藏" : value ? "显示输入的内容" : "查看已保存的 API Key(可能需要钥匙串授权)"}
        disabled={!canToggle || loading}
        className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-1 text-[var(--fg-faint)] hover:text-[var(--fg)] disabled:opacity-35"
        onClick={() => void toggle()}
      >
        {showing ? <EyeOffIcon size={15} /> : <EyeIcon size={15} />}
      </button>
    </div>
  );
}
