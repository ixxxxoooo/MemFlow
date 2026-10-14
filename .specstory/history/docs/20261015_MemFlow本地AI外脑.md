# MemFlow:本地 AI 第二大脑

> @author ygw　　日期:2026-10-15

## 一、产品定位

重输入、轻管理、AI 驱动的本地外脑。随手记(文字/截图/文件)→ 本地 Ollama 自动整理 → 以对话方式取回,所有回答带可追溯来源。数据全部保存在本机。

## 二、使用说明

### 1. 准备模型
需要本机运行 Ollama,并安装:

| 用途 | 模型 | 说明 |
| --- | --- | --- |
| 向量 | `bge-m3` | 必需 |
| 文本 | `qwen2.5:7b` | 必需 |
| 视觉 | `minicpm-v` | 可选,默认关闭,在「设置」中开启 |

应用内顶部横幅提供「一键下载」。若你的系统代理使用 fake-ip(`198.18.x.x`),`ollama pull` 会报 `redirect target not allowed`,请改用:

```bash
scripts/pull_model_manual.sh bge-m3 latest
scripts/pull_model_manual.sh qwen2.5 7b
```

### 2. 闪念胶囊
- 全局快捷键默认 `Cmd+Shift+K`(可在设置修改)。
- 直接输入文字,Enter 保存;可粘贴截图、拖入文件(txt/md/pdf/docx/图片)。
- Esc 收起,草稿保留 5 分钟;失焦自动收起;提交后右下角出现轻提示。
- 托盘常驻,关闭主窗口只会隐藏。

### 3. 主工作台
- **AI 问答**:只依据你的记录回答,`[n]` 引用可点击,右侧抽屉查看原文;无相关资料时固定拒答,不调用模型;回答中的 Mermaid 流程图支持缩放拖拽、查看代码、导出 SVG/PNG。
- **记忆时间流**:按时间/标签/类型筛选与搜索,可编辑、删除、重新处理。
- **知识档案库**:导入的文档,含全局摘要与分块。

### 4. 重刷(重新索引)
更换向量模型或提示词版本后,顶部横幅提示「向量过期」,一键重刷;重刷在单事务内替换衍生数据,不产生重复切片与脏 FTS。

## 三、架构要点

- Tauri 2 多窗口(main / capsule / toast),Rust 后端经 `EventSink` 与 UI 解耦,便于测试。
- SQLite(WAL)+ FTS5(jieba 分词)+ 内存暴力余弦向量索引(按 embed_model 隔离)。
- 检索:向量 + FTS → RRF(k=60) → 笔记/图片时效加权 → 相关度门槛 → 每条记录至多 2 块。
- 健壮性:
  - 原始数据先落盘,AI 处理异步可重试;
  - 持久化任务队列 + 崩溃恢复;
  - Ollama 离线或模型缺失 30 秒后重排且不消耗重试次数,永久错误立即失败,其他错误指数退避(上限 5 次);
  - 结构化提取失败重试 3 次后降级为「原文 + 向量」;
  - 引用 `[n]` 校验,非法引用自动剔除;
  - 数据库升级前自动备份。
- 数据目录:`~/Library/Application Support/MemFlow/`(`memflow.db`、`attachments/`、`tmp/`、`backups/`)。

## 四、开发与测试

```bash
pnpm install
pnpm tauri dev        # 开发运行
cargo test            # 在 src-tauri 下,55 单测 + 9 个 mock Ollama 集成测试
pnpm test             # 前端 vitest
pnpm tauri build      # 产出 .app / .dmg
```

## 五、已知限制

- 胶囊在其他应用的全屏 Space 上可能不显示(未使用 NSPanel)。
- 扫描版 PDF 无 OCR;视觉模型默认关闭。
- 构建产物为 ad-hoc 签名;分发给他人需 Apple 开发者证书签名并公证。
