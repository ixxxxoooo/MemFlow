<div align="center">

<img src="design/icon.png" width="120" alt="MemFlow" />

# MemFlow

**本地优先的 Mac AI 第二大脑 · 重输入、轻管理**

随手记下一句话、一张截图、一份文档,本地 Ollama 在后台自动理解、打标签、向量化;需要时用对话取回,回答 100% 带可追溯来源。

[![CI](https://github.com/ixxxxoooo/MemFlow/actions/workflows/ci.yml/badge.svg)](https://github.com/ixxxxoooo/MemFlow/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ixxxxoooo/MemFlow)](https://github.com/ixxxxoooo/MemFlow/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%2012%2B-lightgrey)

[English](#english) · [下载](#下载与安装) · [快速开始](#快速开始) · [贡献](CONTRIBUTING.md)

</div>

## 特性

- **闪念胶囊与记一笔**:全局快捷键(默认 `Cmd+Shift+K`)唤起 Spotlight 风格输入框;回车保存、`Cmd+V` 粘贴截图、拖入文件;失焦自动收起,托盘常驻。主界面侧栏有常驻的「记一笔」按钮(`⌘N`),两者都存入默认知识库。
- **全本地(默认)**:嵌入 `bge-m3`、文本 `qwen2.5:7b`、视觉 `minicpm-v`(默认关闭),数据与推理都不出本机。
- **可选云端模型**:文本、向量、视觉模型都可以分别选择本地 Ollama 或云端 OpenAI 兼容接口(如 DeepSeek、通义、硅基流动),全部选云端即可免下载本地模型;原始数据与向量索引始终留在本机(见下文隐私说明)。
- **混合检索**:向量 + FTS5(jieba 中文分词)→ RRF 融合 → 本地 ONNX 重排序(可选,设置页一键下载)→ 时效加权 → 相关度门槛。
- **可溯源问答**:只依据你的记录回答,`[n]` 角标可点击,右侧抽屉查看原文;无相关资料时直接拒答,不编造。
- **Mermaid 流程图**:校验 → 自动修复 → 缩放拖拽 → 导出 SVG/PNG。
- **文档导入**:txt / md / pdf / docx / pptx / xlsx / csv / html / epub / rtf(详见下方格式表),自动分块并生成全局摘要;macOS 上扫描版 PDF 经本机 OCR、表格还原为 Markdown,来源标注页码;多文档并发入库。
- **多知识库**:「知识档案库」先是知识库列表(新建、改名与描述、删除),点进去才是库内文档;导入固定到当前打开的库。提问时在输入框左上方的范围选择器里多选知识库(不选 = 搜索全部),文档可批量转移到其他库。导入入口会提示支持的格式。
- **图文关联**:文档里的图片自动入库并关联,本机 OCR 识别图中文字并入对应切片,搜图中的文字也能命中文档。
- **AI 整理**:单条笔记一键整理(预览、确认、可撤销);按语义相似度把零散碎片归并成带来源编号的主题笔记,原记录保留。
- **问答中切换模型**:输入框旁一键切换本地 Ollama 或多个云端模型,每条回答显示模型图标、名称、总耗时与首字耗时。
- **健壮**:原始数据先落盘、AI 处理异步可重试;持久化任务队列 + 崩溃恢复;Ollama 离线不丢数据;一键重刷。

## 下载与安装

前往 [Releases](https://github.com/ixxxxoooo/MemFlow/releases/latest) 下载对应架构的 `.dmg`:

| 芯片 | 文件 |
| --- | --- |
| Apple Silicon (M 系列) | `MemFlow_<版本>_aarch64.dmg` |
| Intel | `MemFlow_<版本>_x64.dmg` |

每个版本附带 `SHA256SUMS.txt`,可用 `shasum -a 256 -c SHA256SUMS.txt` 校验。

> **关于签名**:当前发布包使用 ad-hoc 签名,未经过 Apple 公证。首次打开如被 Gatekeeper 拦截,请在「访达」中右键 → 打开,或执行:
>
> ```bash
> xattr -cr /Applications/MemFlow.app
> ```

## 支持的文档格式

| 格式 | 解析方式 |
| --- | --- |
| `.md` `.markdown` `.txt` | 直接读取;md 里的相对路径图片自动关联 |
| `.pdf` | 文本层 + 扫描页本机 OCR(macOS)、表格还原为 Markdown、页码;可选视觉模型 |
| `.docx` | 标题层级、表格(含合并单元格、嵌套表格)、内嵌图片 |
| `.pptx` | 按幻灯片提取标题/正文/备注,幻灯片图片自动关联 |
| `.xlsx` | 每个工作表转 Markdown 表格(识别日期),大表分块时重复表头 |
| `.csv` `.tsv` | 自动识别 UTF-8 / GBK / UTF-16,转 Markdown 表格 |
| `.html` `.htm` | 提取正文与标题层级,`<img>` 相对路径图片自动关联 |
| `.epub` | 按目录顺序提取章节,章节插图自动关联 |
| `.rtf` | 提取文字(含中文代码页) |
| 图片 `.png` `.jpg` `.webp` `.gif` `.bmp` | 本机 OCR 识别图中文字;可选视觉模型描述 |

暂不支持旧版 `.doc` / `.ppt` / `.xls`(请先另存为新格式)。导入文件夹时会递归处理以上格式,并跳过隐藏文件与 `node_modules`、`.git` 等目录。

## 快速开始

1. 安装并启动 [Ollama](https://ollama.com)。
2. 打开 MemFlow,顶部横幅会提示缺失的模型,点击「一键下载」(`bge-m3` 约 1.2GB,`qwen2.5:7b` 约 4.7GB)。
   - 若系统代理(如 Quantumult X)使用 fake-ip(`198.18.x.x`)导致 `ollama pull` 报 `redirect target not allowed`,在「设置 → 模型状态与下载」点「**直连下载**」即可;也可用终端脚本 [`scripts/pull_model_manual.sh`](scripts/pull_model_manual.sh):
     ```bash
     scripts/pull_model_manual.sh qwen2.5:7b
     scripts/pull_model_manual.sh bge-m3
     # 网络受限时可指定镜像仓库
     OLLAMA_REGISTRY=https://你的镜像地址 scripts/pull_model_manual.sh qwen2.5:7b
     ```
3. 按 `Cmd+Shift+K` 记下第一条想法,然后到「AI 问答」里提问。

### 圈x(Quantumult X)用户:让 Ollama 域名不走 fake-ip

**现象**:`ollama pull` 报 `redirect target not allowed`,或下载一直失败。

**原因**:Ollama 的模型文件先请求 `registry.ollama.ai`,再 307 跳转到 `*.r2.cloudflarestorage.com` 的 CDN。圈x 开启 fake-ip 后,这些域名会被解析成 `198.18.0.0/15` 的假地址,Ollama 判定为不安全的跳转目标而拒绝。

**配置**:在圈x 的配置文件里找到 `[general]`,加入(已有 `dns_exclusion_list` 时,把下面的域名用逗号追加进去,不要写两行):

```ini
[general]
dns_exclusion_list = *.ollama.ai, ollama.ai, *.ollama.com, ollama.com, *.r2.cloudflarestorage.com
```

保存后在圈x 里重新加载配置(或重启圈x)。该项的含义是「这些域名不使用 fake-ip,走真实 DNS」,是否走代理仍由你的分流规则决定,可按需再补规则,例如:

```ini
[filter_local]
host-suffix, ollama.ai, proxy
host-suffix, r2.cloudflarestorage.com, proxy
```

(`proxy` 换成你自己的策略组名;若直连网络就能访问,写 `direct`。)

**验证**:

```bash
nslookup registry.ollama.ai          # 返回的 IP 不应在 198.18.x.x 范围
curl -I https://registry.ollama.ai/v2/   # 能返回 HTTP 状态码即连通
```

**兜底**:不想改圈x 配置时,在 MemFlow「设置 → 模型状态与下载」使用「直连下载」,它由应用自己下载并校验模型文件,不依赖上面的配置。

### 使用云端模型(可选)

在「设置 → 文本模型来源」选择「云端 API」,挑选服务商预设(如 DeepSeek),填入 API Key,点「测试连接」后保存即可。API Key 输入框右侧的眼睛图标可查看明文(已保存的密钥需要钥匙串授权,10 秒后自动隐藏)。

- 文本模型:问答、查询改写、自动打标签、流程图修复使用云端模型。
- 向量模型与视觉模型:在「设置 → 向量与视觉模型」分别选择「本地(Ollama)」或「云端(OpenAI 兼容)」,云端时选一个已配置的云端档案并填模型名(如向量 `text-embedding-v3` / `BAAI/bge-m3`,视觉 `qwen-vl-plus`),可点「测试」验证。DeepSeek 官方没有 embedding 接口,向量请选通义、硅基流动、OpenAI 等。
- **更换向量来源后旧向量不再参与检索**(不同模型的向量空间不兼容),顶部会提示「全库重刷」。
- **隐私**:云端文本模型会收到你的提问与检索到的资料片段;云端向量模型会收到笔记与文档的文本片段;云端视觉模型会收到图片与 PDF 页面图像。原始数据和向量索引仍只保存在本机。资料敏感时请使用本地模型。
- API Key 保存在 macOS 钥匙串,不写入数据库,也不会包含在备份中。

### 应用内快捷键

| 快捷键 | 功能 |
| --- | --- |
| `⌘ ,` | 打开设置 |
| `⌘ N` | 记一笔 |
| `⌘ 1` / `⌘ 2` / `⌘ 3` | AI 问答 / 记忆时间流 / 知识档案库 |

## 从源码构建

环境要求:macOS 12+、Rust stable、Node.js ≥ 20、pnpm。

```bash
pnpm install
pnpm tauri dev        # 开发运行
pnpm tauri build      # 产出 .app 与 .dmg(位于 src-tauri/target/release/bundle)

# 测试
pnpm test                         # 前端 vitest
cd src-tauri && cargo test        # Rust 单元 + mock Ollama 集成测试
# 真机冒烟(需本机 Ollama 与已下载模型)
cd src-tauri && cargo test --test e2e_real_ollama -- --ignored --nocapture
```

## 架构概览

Tauri 2 多窗口(main / capsule / toast)+ Rust 后端 + React 19 前端。后端经 `EventSink` 与 UI 解耦,可在无窗口环境下完整测试。

- 存储:SQLite(WAL)+ FTS5 + 内存余弦向量索引(按嵌入模型隔离),数据位于 `~/Library/Application Support/MemFlow/`。
- 队列:持久化任务、崩溃恢复、指数退避;Ollama 离线或模型缺失仅重排不耗次数。
- 多知识库:`items.kb_id` + 向量块 kb_id,检索与向量扫描都在范围内硬过滤;转移只改归属,无需重新嵌入。
- 图文关联:`item_links` 记录文档与图片的多对多关系,图中文字(本机 OCR)并入所在切片。
- AI 整理:`item_revisions` 保存旧版本支持撤销,归并用已有向量聚类并记录 `merged_from` 来源。
- 问答:无命中拒答 → 带编号资料 → 流式生成 → 引用校验(剔除非法 `[n]`)→ 会话落库。

数据位置、与 RAGFlow 的逐环节对比、性能调优建议见 [`docs/架构与RAGFlow对比.md`](docs/架构与RAGFlow对比.md)。

更多细节见 [`.specstory/history/docs/20261015_MemFlow本地AI外脑.md`](.specstory/history/docs/20261015_MemFlow本地AI外脑.md)。

## 已知限制

- 胶囊在其他应用的全屏 Space 上可能不显示(未使用 NSPanel)。
- PDF 的 OCR / 表格还原目前仅 macOS 可用(PDFKit + Vision);其它平台回退为纯文本层。文本层 PDF 中的表格仅做段落重排,需要高精度表格可在设置里切换「视觉模型」解析模式。视觉模型默认关闭。
- 图中文字的本机 OCR 仅 macOS 可用;其它平台只保留图片与文档的关联,不识别图中文字。
- AI 整理只支持文字笔记(文档由原文件生成,不做覆盖式整理);归并一次最多 12 条、总计 2.4 万字,需在同一知识库内。
- 发布包为 ad-hoc 签名,未公证(见上文)。

## 参与贡献

欢迎 Issue 与 PR!请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 与 [行为准则](CODE_OF_CONDUCT.md)。安全问题请见 [SECURITY.md](SECURITY.md)。

## 许可证

[MIT](LICENSE) © 2026 ixxxxoooo

---

## English

**MemFlow** is a local-first "AI second brain" for macOS. Capture a thought, a screenshot or a document in one keystroke; a local [Ollama](https://ollama.com) instance understands, tags and indexes it in the background. Ask questions later and get answers with 100% traceable citations - or an honest refusal when nothing relevant is stored.

- Global quick-capture capsule (`Cmd+Shift+K`), tray resident
- Fully local: `bge-m3` embeddings + `qwen2.5:7b` chat (+ optional `minicpm-v` vision)
- Hybrid retrieval (vectors + FTS5 with Chinese tokenization, RRF fusion)
- Verified `[n]` citations, Mermaid flowcharts with auto-repair and SVG/PNG export
- Robust by design: raw data is persisted first, AI processing is async, retryable and crash-safe

**Install**: download the `.dmg` for your chip from [Releases](https://github.com/ixxxxoooo/MemFlow/releases/latest). Builds are ad-hoc signed (not notarized); if Gatekeeper blocks the app, right-click → Open or run `xattr -cr /Applications/MemFlow.app`.

**Build**: `pnpm install && pnpm tauri build` (requires Rust stable, Node ≥ 20, pnpm, macOS 12+).

Licensed under the [MIT License](LICENSE).
