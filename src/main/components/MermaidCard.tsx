// @author ygw
// Mermaid 流程图卡片:校验 → 自动修复 → 渲染;支持放大、查看代码、导出 SVG/PNG、复制代码

import { save } from "@tauri-apps/plugin-dialog";
import mermaid from "mermaid";
import { useEffect, useId, useState } from "react";
import { api } from "../../lib/api";
import { toBase64 } from "../../lib/format";
import { renderWithRepair } from "../../lib/mermaidRepair";
import { useApp } from "../../lib/store";
import { CloseIcon, CodeIcon, CopyIcon, DownloadIcon, ZoomIcon } from "./icons";
import { PanZoom } from "./PanZoom";

let initialized = false;

/** 初始化 mermaid:关闭 htmlLabels 以避免导出 PNG 时 WebKit 画布被污染 */
function ensureInit() {
  if (initialized) return;
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: dark ? "dark" : "default",
    htmlLabels: false,
    flowchart: { htmlLabels: false, curve: "basis" },
    fontFamily: '-apple-system, "PingFang SC", sans-serif',
  } as Parameters<typeof mermaid.initialize>[0]);
  initialized = true;
}

/** 渲染 mermaid 代码为 SVG 字符串(先 parse 校验,失败抛异常) */
async function renderSvg(id: string, code: string): Promise<string> {
  ensureInit();
  await mermaid.parse(code);
  const { svg } = await mermaid.render(id, code);
  return svg;
}

/** 从 SVG 字符串读取尺寸 */
function svgSize(svg: string): { w: number; h: number } {
  const vb = svg.match(/viewBox="([\d.\-\s]+)"/);
  if (vb) {
    const p = vb[1].trim().split(/\s+/).map(Number);
    if (p.length === 4 && p[2] > 0 && p[3] > 0) return { w: p[2], h: p[3] };
  }
  return { w: 1200, h: 800 };
}

/** SVG → PNG(base64,无前缀) */
async function svgToPngBase64(svg: string): Promise<string> {
  const { w, h } = svgSize(svg);
  const scale = 2;
  const img = new Image();
  const src = `data:image/svg+xml;base64,${toBase64(svg)}`;
  await new Promise<void>((resolve, reject) => {
    img.onload = () => resolve();
    img.onerror = () => reject(new Error("图片渲染失败"));
    img.src = src;
  });
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(w * scale);
  canvas.height = Math.ceil(h * scale);
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("无法创建画布");
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  ctx.fillStyle = dark ? "#1c1c20" : "#ffffff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/png").split(",")[1];
}

interface Props {
  code: string;
  /** 正在流式生成中:先不渲染,仅展示占位 */
  live?: boolean;
}

export function MermaidCard({ code, live }: Props) {
  const uid = useId().replace(/:/g, "");
  const toast = useApp((s) => s.toast);
  const [svg, setSvg] = useState<string>("");
  const [finalCode, setFinalCode] = useState(code);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [repairs, setRepairs] = useState(0);
  const [showCode, setShowCode] = useState(false);
  const [zoom, setZoom] = useState(false);

  /** 渲染并按需自动修复 */
  const run = async (src: string) => {
    setBusy(true);
    setError("");
    let n = 0;
    const out = await renderWithRepair(
      src,
      (c) => renderSvg(`mf-${uid}-${n++}`, c),
      (c, e) => api.fixMermaid(c, e),
      2,
    );
    setBusy(false);
    setFinalCode(out.code);
    setRepairs(out.repairs);
    if (out.ok && out.svg) setSvg(out.svg);
    else {
      setSvg("");
      setError(out.error ?? "渲染失败");
      setShowCode(true);
    }
  };

  useEffect(() => {
    if (live) return;
    void run(code);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code, live]);

  const exportAs = async (kind: "svg" | "png") => {
    try {
      const path = await save({ defaultPath: `流程图.${kind}`, filters: [{ name: kind.toUpperCase(), extensions: [kind] }] });
      if (!path) return;
      const data = kind === "svg" ? toBase64(svg) : await svgToPngBase64(svg);
      await api.writeExport(path, data);
      toast(`已导出到 ${path}`, "ok");
    } catch (e) {
      toast(`导出失败:${(e as Error).message}`, "err");
    }
  };

  const copyCode = async () => {
    try {
      await navigator.clipboard.writeText(finalCode);
      toast("已复制 Mermaid 代码", "ok");
    } catch {
      toast("复制失败", "err");
    }
  };

  const btn = "flex items-center gap-1 rounded-md px-2 py-1 text-xs text-[var(--fg-dim)] hover:bg-[var(--bg-soft)] hover:text-[var(--fg)] disabled:opacity-40";

  return (
    <div className="my-3 overflow-hidden rounded-xl border border-[var(--line)] bg-[var(--bg-elev)]">
      <div className="flex items-center justify-between border-b border-[var(--line)] px-3 py-1.5">
        <span className="text-xs font-medium text-[var(--fg-dim)]">
          流程图{repairs > 0 && !error ? <span className="ml-2 rounded bg-[var(--accent-soft)] px-1.5 py-0.5 text-[10px] text-[var(--accent)]">已自动修复语法</span> : null}
        </span>
        <div className="flex items-center gap-0.5">
          <button className={btn} disabled={!svg} onClick={() => setZoom(true)}><ZoomIcon size={13} />放大</button>
          <button className={btn} onClick={() => setShowCode((v) => !v)}><CodeIcon size={13} />{showCode ? "隐藏代码" : "查看代码"}</button>
          <button className={btn} disabled={!svg} onClick={() => exportAs("svg")}><DownloadIcon size={13} />SVG</button>
          <button className={btn} disabled={!svg} onClick={() => exportAs("png")}><DownloadIcon size={13} />PNG</button>
          <button className={btn} onClick={copyCode}><CopyIcon size={13} /></button>
        </div>
      </div>
      <div className="p-3">
        {live ? (
          <div className="flex h-24 items-center justify-center text-sm text-[var(--fg-dim)]">
            <span className="spin mr-2 inline-block h-3 w-3 rounded-full border-2 border-[var(--accent)] border-t-transparent" />
            图表生成中…
          </div>
        ) : busy ? (
          <div className="flex h-24 items-center justify-center text-sm text-[var(--fg-dim)]">
            <span className="spin mr-2 inline-block h-3 w-3 rounded-full border-2 border-[var(--accent)] border-t-transparent" />
            正在渲染 / 自动修复语法…
          </div>
        ) : svg ? (
          <div className="flex max-h-[420px] justify-center overflow-auto" dangerouslySetInnerHTML={{ __html: svg }} />
        ) : error ? (
          <div className="rounded-lg bg-[var(--bg-soft)] p-3 text-xs text-[var(--err)]">
            图表语法有误,自动修复未成功:{error}
            <div className="mt-2">
              <button className="rounded-md bg-[var(--accent-soft)] px-2 py-1 text-[var(--accent)]" onClick={() => void run(finalCode)}>再试一次</button>
            </div>
          </div>
        ) : null}
        {showCode && (
          <pre className="selectable mt-3 max-h-60 overflow-auto rounded-lg bg-[var(--bg-soft)] p-3 text-xs"><code>{finalCode}</code></pre>
        )}
      </div>
      {zoom && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-8" onClick={() => setZoom(false)}>
          <div className="relative h-full w-full max-w-6xl overflow-hidden rounded-2xl border border-[var(--line)] bg-[var(--bg)] shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <button className="absolute right-3 top-3 z-10 rounded-full bg-[var(--bg-soft)] p-1.5" onClick={() => setZoom(false)}><CloseIcon size={14} /></button>
            <PanZoom className="h-full w-full">
              <div dangerouslySetInnerHTML={{ __html: svg }} />
            </PanZoom>
          </div>
        </div>
      )}
    </div>
  );
}
