// @author ygw
//! 记录处理流水线:文档解析 → (图片)视觉解析 → 结构化提取 → 分块 → 向量化 → 事务写入。
//!
//! 健壮性约定:
//! - 原始数据早已落盘,本流水线只生成可再生的"派生数据"
//! - 派生数据在内存中完整生成后,于单个事务内整体替换旧数据;中途失败旧索引不受影响
//! - 结构化提取解析失败会重试,仍失败则降级为"仅原文 + 向量"

use crate::chunker::{self, ChunkOptions, ChunkPiece};
use crate::core::{permit, CancelToken, Core, EMBED_CONCURRENCY};
use crate::error::{AppError, Result};
use crate::extract;
use crate::images;
use crate::ollama::OllamaClient;
use crate::pdf_ingest;
use crate::prompts::{self, ExtractedMeta, VisionResult, PROMPT_VERSION};
use crate::repo::{self, ItemView, NewChunk, NewMeta};
use crate::tokenizer;
use crate::vector::{self, Entry};
use futures_util::stream::{self, StreamExt, TryStreamExt};
use serde_json::json;
use std::path::PathBuf;

/// 非文档类记录不超过该字符数时不再分块
const SINGLE_CHUNK_MAX: usize = 1200;
/// 每批向量化的切片数
const EMBED_BATCH: usize = 16;
/// 提取输入的最大字符数(过长文档采样)
const EXTRACT_INPUT_MAX: usize = 6000;
/// 图片大小上限
const MAX_IMAGE_BYTES: u64 = 30 * 1024 * 1024;

/// 对长文本采样:保留开头、中段与结尾,以便摘要覆盖全文
///
/// 参数: text 全文;max 最大字符数
/// 返回: 采样后的文本
pub fn sample_text(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let head = max * 6 / 10;
    let mid = max * 2 / 10;
    let tail = max - head - mid;
    let mid_start = chars.len() / 2 - mid / 2;
    let s = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    format!(
        "{}\n……\n{}\n……\n{}",
        s(0, head),
        s(mid_start, mid_start + mid),
        s(chars.len() - tail, chars.len())
    )
}

/// 清洗并限制提取结果
///
/// 参数: m 模型原始输出
/// 返回: 规整后的元数据
pub fn clean_meta(mut m: ExtractedMeta) -> ExtractedMeta {
    let clip = |s: &str, n: usize| -> String { s.trim().chars().take(n).collect() };
    m.summary = clip(&m.summary, 400);
    m.category = clip(&m.category, 12);
    let mut seen = std::collections::HashSet::new();
    m.tags = m
        .tags
        .iter()
        .map(|t| clip(t.trim().trim_start_matches('#'), 20))
        .filter(|t| !t.is_empty() && seen.insert(t.to_lowercase()))
        .take(6)
        .collect();
    let mut seen = std::collections::HashSet::new();
    m.entities = m
        .entities
        .iter()
        .map(|t| clip(t, 30))
        .filter(|t| !t.is_empty() && seen.insert(t.to_lowercase()))
        .take(10)
        .collect();
    m
}

/// 判断任务模式是否要求 AI 解读(ingest_ai / reindex_ai)
///
/// 参数: mode 队列任务模式
/// 返回: 要求 AI 解读返回 true
pub fn is_ai_mode(mode: &str) -> bool {
    mode.ends_with("_ai")
}

/// 本地快速元数据:不调用模型,摘要取正文开头一段有效文字
///
/// 参数: text 全文
/// 返回: 只含摘要的元数据(无标签、无实体)
pub fn local_meta(text: &str) -> ExtractedMeta {
    let summary: String = text
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect();
    ExtractedMeta {
        summary,
        ..Default::default()
    }
}

/// 构造用于向量化的输入文本
///
/// 参数: item 记录;meta 元数据;piece 切片
/// 返回: 向量化输入
pub fn build_embed_input(
    kind: &str,
    title: &str,
    meta: &Option<ExtractedMeta>,
    piece: &ChunkPiece,
) -> String {
    let mut s = String::new();
    if kind == "document" {
        if !title.is_empty() {
            s.push_str(&format!("《{title}》\n"));
        }
        if let Some(m) = meta.as_ref().filter(|m| !m.summary.is_empty()) {
            s.push_str(&format!("摘要:{}\n", m.summary));
        }
        if !piece.heading.is_empty() {
            s.push_str(&format!("[{}]\n", piece.heading));
        }
        s.push_str(&piece.text);
    } else {
        s.push_str(&piece.text);
        if let Some(m) = meta.as_ref().filter(|m| !m.tags.is_empty()) {
            s.push_str(&format!("\n标签:{}", m.tags.join(" ")));
        }
    }
    s
}

/// 判断模型是否已安装(名称匹配忽略 :latest 后缀)
///
/// 参数: installed 已安装模型名列表;want 期望模型名
/// 返回: 是否已安装
pub fn model_installed(installed: &[String], want: &str) -> bool {
    let norm = |s: &str| s.strip_suffix(":latest").unwrap_or(s).to_string();
    let w = norm(want);
    installed.iter().any(|n| norm(n) == w)
}

/// 结构化提取(带重试与降级)
///
/// 参数: core 核心;ollama 客户端;text 文本;time_label 时间文案;is_doc 是否文档
/// 返回: Some 为成功;None 表示重试后仍无法解析(降级)
async fn extract_meta(
    core: &Core,
    text: &str,
    time_label: &str,
    is_doc: bool,
    token: &CancelToken,
) -> Result<Option<ExtractedMeta>> {
    // 本地模型串行、云端模型可并发
    let stage = if core.settings().uses_cloud_llm() {
        &core.stages.llm_cloud
    } else {
        &core.stages.llm_local
    };
    let _permit = permit(stage).await?;
    core.prepare_text_model().await;
    let llm = core.llm();
    let text_model = core.text_model();
    let msgs = prompts::extract_messages(&sample_text(text, EXTRACT_INPUT_MAX), time_label, is_doc);
    let schema = prompts::extract_schema();
    for attempt in 0..3 {
        if token.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let temp = if attempt == 0 { 0.1 } else { 0.4 };
        match llm
            .chat_json::<ExtractedMeta>(&text_model, &msgs, &core.gen_opts(temp), &schema)
            .await
        {
            Ok(m) => return Ok(Some(clean_meta(m))),
            Err(AppError::Parse(e)) => {
                tracing::warn!("结构化提取解析失败(第{}次): {e}", attempt + 1)
            }
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

/// 视觉解析(带重试)
///
/// 本地视觉模型与本地文本模型共用串行许可并做内存调度;云端视觉模型走云端并发许可,不需要本地模型。
async fn run_vision(
    core: &Core,
    ollama: &OllamaClient,
    item: &ItemView,
    abs: &PathBuf,
    token: &CancelToken,
) -> Result<Option<VisionResult>> {
    let s = core.settings();
    let (client, model) = core.vision_backend()?;
    let cloud = client.is_cloud();
    // 本地模型串行占用内存;云端模型只受云端并发上限约束
    let _permit = if cloud {
        permit(&core.stages.llm_cloud).await?
    } else {
        permit(&core.stages.llm_local).await?
    };
    if !cloud {
        let installed: Vec<String> = ollama
            .list_models()
            .await?
            .into_iter()
            .map(|m| m.name)
            .collect();
        if !model_installed(&installed, &s.vision_model) {
            return Err(AppError::ModelMissing(s.vision_model.clone()));
        }
    }
    let meta = tokio::fs::metadata(abs).await?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(AppError::Invalid("图片超过 30MB,无法解析".into()));
    }
    let bytes = tokio::fs::read(abs).await?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    if !cloud {
        core.prepare_chat_model(&model).await;
    }
    let schema = prompts::vision_schema();
    for attempt in 0..3 {
        if token.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let msgs = prompts::vision_messages(&item.content, b64.clone());
        let temp = if attempt == 0 { 0.1 } else { 0.4 };
        match client
            .chat_json::<VisionResult>(&model, &msgs, &core.gen_opts(temp), &schema)
            .await
        {
            Ok(v) => return Ok(Some(v)),
            Err(AppError::Parse(e)) => {
                tracing::warn!("视觉解析 JSON 失败(第{}次): {e}", attempt + 1)
            }
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

/// 时间文案,如 "2026-10-14 周三 21:30"
fn time_label(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%Y-%m-%d %a %H:%M").to_string())
        .unwrap_or_default()
}

/// 处理一条记录
///
/// 参数: core 核心;item_id 记录 id;mode ingest/reindex;token 取消令牌
/// 返回: 成功返回 Ok;记录已被删除视为成功
pub async fn process_item(
    core: &std::sync::Arc<Core>,
    item_id: i64,
    mode: &str,
    token: &CancelToken,
) -> Result<()> {
    let Some(item) = core.db.with_read(|c| repo::get_item(c, item_id))? else {
        return Ok(());
    };
    core.db
        .with(|c| repo::set_item_status(c, item_id, "processing", None))?;
    core.emit("item-updated", json!({ "id": item_id }));
    let s = core.settings();
    let ollama = core.ollama();
    tracing::info!("处理记录 {item_id} ({}, mode={mode})", item.kind);

    // ---- 阶段 1:准备正文 ----
    let abs = item
        .attachment_path
        .as_ref()
        .map(|p| core.paths.attachments.join(p));
    let mut content = item.content.clone();
    if item.kind == "document" && content.trim().is_empty() {
        let path = abs
            .clone()
            .ok_or_else(|| AppError::Invalid("文档缺少附件".into()))?;
        content = if extract::ext_lower(&path) == "pdf" {
            // PDF 逐页解析,内部按页获取 CPU 许可(OCR 可能很久,不能整段占用)
            pdf_ingest::extract_pdf(core, &path, token).await?
        } else {
            let _cpu = permit(&core.stages.cpu).await?;
            extract::extract_text(&path).await?
        };
        core.db
            .with(|c| repo::update_item_text(c, item_id, None, &content))?;
    }
    if token.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    // ---- 阶段 2:图片视觉解析(可选) ----
    let mut vision_text = item.vision_text.clone();
    let mut vision: Option<VisionResult> = None;
    if item.kind == "image" && s.vision_enabled {
        let path = abs
            .clone()
            .ok_or_else(|| AppError::Invalid("图片缺少附件".into()))?;
        vision = run_vision(core, &ollama, &item, &path, token).await?;
        if let Some(v) = &vision {
            let mut t = v.description.trim().to_string();
            if !v.ocr_text.trim().is_empty() {
                t.push_str(&format!("\n图中文字:{}", v.ocr_text.trim()));
            }
            if !v.intent.trim().is_empty() {
                t.push_str(&format!("\n意图:{}", v.intent.trim()));
            }
            core.db.with(|c| repo::set_vision_text(c, item_id, &t))?;
            vision_text = Some(t);
        }
    }
    // 图片的本机 OCR(系统 Vision,不依赖模型):让图中文字可被文字搜索命中
    let mut ocr_text: Option<String> = None;
    if item.kind == "image" {
        if let Some(rel) = item.attachment_path.as_deref() {
            let local = images::ensure_ocr(core, item_id, rel).await;
            // 本机识别不到时,退回视觉模型识别出的文字
            let t = if local.trim().is_empty() {
                vision
                    .as_ref()
                    .map(|v| v.ocr_text.trim().to_string())
                    .unwrap_or_default()
            } else {
                local
            };
            ocr_text = Some(t);
        }
    }
    if token.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    // ---- 阶段 3:确定可索引文本 ----
    let index_text = match item.kind.as_str() {
        "image" => {
            let mut parts: Vec<String> = Vec::new();
            if !content.trim().is_empty() {
                parts.push(content.trim().to_string());
            }
            if let Some(v) = vision_text.as_ref().filter(|v| !v.trim().is_empty()) {
                parts.push(v.trim().to_string());
            }
            // 视觉模型文本里已包含的文字不重复追加
            if let Some(o) = ocr_text.as_ref().filter(|o| !o.trim().is_empty()) {
                let first = o.lines().next().unwrap_or("").trim();
                let dup = vision_text
                    .as_ref()
                    .map(|v| v.contains(first))
                    .unwrap_or(false);
                if !dup {
                    parts.push(format!("图中文字:{}", o.trim()));
                }
            }
            if parts.is_empty() {
                // 没有任何文字线索:仅归档,不产生索引
                core.db.with_tx(|tx| {
                    repo::clear_chunks(tx, item_id)?;
                    repo::set_item_status(tx, item_id, "archived_only", None)
                })?;
                core.vindex.remove_item(item_id);
                return Ok(());
            }
            parts.join("\n")
        }
        // 文档:把引用的图片文字并入对应位置(同时保证每张图都已完成本机 OCR)
        "document" => {
            let mut links = core.db.with_read(|c| repo::images_of(c, item_id))?;
            for l in links.iter().filter(|l| l.ocr_text.is_none()) {
                if let Some(rel) = l.attachment_path.as_deref() {
                    images::ensure_ocr(core, l.child_id, rel).await;
                }
            }
            if links.iter().any(|l| l.ocr_text.is_none()) {
                links = core.db.with_read(|c| repo::images_of(c, item_id))?;
            }
            images::inline_images(&content, &links)
        }
        _ => content.clone(),
    };

    // ---- 阶段 4:结构化提取 ----
    let tl = time_label(item.created_at);
    // 文档默认只做快速索引(不调用 LLM);设置开启或任务显式要求 AI 时才解读
    let use_ai = item.kind != "document" || s.doc_ai_enrich || is_ai_mode(mode);
    let mut ai_used = false;
    // 提取元数据时不带页标记
    let plain_text = images::strip_image_marks(&chunker::strip_page_marks(&index_text));
    let meta: Option<ExtractedMeta> = if let Some(v) = vision.as_ref() {
        ai_used = true;
        Some(clean_meta(ExtractedMeta {
            summary: v.description.chars().take(120).collect(),
            category: v.category.clone(),
            tags: v.tags.clone(),
            entities: vec![],
        }))
    } else if !use_ai {
        Some(local_meta(&plain_text))
    } else {
        let m = extract_meta(core, &plain_text, &tl, item.kind == "document", token).await?;
        ai_used = m.is_some();
        m
    };

    // ---- 阶段 5:分块 ----
    let mut pieces: Vec<ChunkPiece> =
        if item.kind != "document" && index_text.chars().count() <= SINGLE_CHUNK_MAX {
            vec![ChunkPiece {
                text: index_text.trim().to_string(),
                heading: String::new(),
                page: None,
            }]
        } else {
            // 长文分块与分词吃 CPU,放到阻塞线程并受 CPU 许可限流
            let _cpu = permit(&core.stages.cpu).await?;
            let text = index_text.clone();
            tokio::task::spawn_blocking(move || chunker::chunk(&text, ChunkOptions::default()))
                .await
                .map_err(|e| AppError::Other(format!("分块任务失败: {e}")))?
        };
    if pieces.is_empty() {
        return Err(AppError::Parse("没有可索引的文本".into()));
    }
    // 记录每个切片里出现的图片,并从切片文本中剔除标记
    let chunk_image_ids = images::take_image_marks(&mut pieces);

    // ---- 阶段 6:向量化 ----
    let title = item
        .title
        .clone()
        .or(item.original_name.clone())
        .unwrap_or_default();
    let inputs: Vec<String> = pieces
        .iter()
        .map(|p| build_embed_input(&item.kind, &title, &meta, p))
        .collect();
    // 批次并发向量化:buffered 保持结果顺序,全局信号量限制 Ollama 同时承受的请求数
    let embedder = core.embedder()?;
    let embed_identity = s.embed_identity();
    let batches: Vec<Vec<String>> = inputs.chunks(EMBED_BATCH).map(|b| b.to_vec()).collect();
    let embedded: Vec<Vec<Vec<f32>>> = stream::iter(batches)
        .map(|batch| {
            let embedder = &embedder;
            async move {
                if token.is_cancelled() {
                    return Err(AppError::Cancelled);
                }
                let _p = permit(&core.stages.embed).await?;
                embedder.embed(&batch).await
            }
        })
        .buffered(EMBED_CONCURRENCY)
        .try_collect()
        .await?;
    let vectors: Vec<Vec<f32>> = embedded.into_iter().flatten().collect();
    let dim = vectors.first().map(|v| v.len()).unwrap_or(0);
    if dim == 0 || vectors.iter().any(|v| v.len() != dim) {
        return Err(AppError::Ollama("向量维度不一致".into()));
    }

    // ---- 阶段 7:事务整体替换 ----
    let tags_text = meta.as_ref().map(|m| m.tags.join(" ")).unwrap_or_default();
    let summary_text = meta.as_ref().map(|m| m.summary.clone()).unwrap_or_default();
    let doc_summary = if item.kind == "document" {
        meta.as_ref()
            .map(|m| m.summary.clone())
            .filter(|x| !x.is_empty())
    } else {
        None
    };
    // 分词与编码向量吃 CPU:放到阻塞线程并受 CPU 许可限流
    let (new_chunks, vectors): (Vec<NewChunk>, Vec<Vec<f32>>) = {
        let _cpu = permit(&core.stages.cpu).await?;
        let (pieces, embed_model, title) = (pieces.clone(), embed_identity.clone(), title.clone());
        let (tags_text, summary_text, doc_summary) =
            (tags_text.clone(), summary_text.clone(), doc_summary.clone());
        tokio::task::spawn_blocking(move || {
            let chunks = pieces
                .iter()
                .zip(vectors.iter())
                .map(|(p, v)| NewChunk {
                    text: p.text.clone(),
                    heading: p.heading.clone(),
                    doc_summary: doc_summary.clone(),
                    embed_model: embed_model.clone(),
                    embedding: vector::encode(v),
                    dim,
                    page: p.page.map(i64::from),
                    fts_tokens: tokenizer::index_tokens(&format!(
                        "{title} {tags_text} {summary_text} {}",
                        p.text
                    )),
                })
                .collect();
            (chunks, vectors)
        })
        .await
        .map_err(|e| AppError::Other(format!("索引构建失败: {e}")))?
    };
    let new_meta = NewMeta {
        summary: meta
            .as_ref()
            .map(|m| m.summary.clone())
            .filter(|x| !x.is_empty()),
        category: meta
            .as_ref()
            .map(|m| m.category.clone())
            .filter(|x| !x.is_empty()),
        tags: meta.as_ref().map(|m| m.tags.clone()).unwrap_or_default(),
        entities: meta
            .as_ref()
            .map(|m| m.entities.clone())
            .unwrap_or_default(),
        vision_text: vision_text.clone(),
        ocr_text: ocr_text.clone(),
        vision_desc: vision
            .as_ref()
            .map(|v| v.description.trim().to_string())
            .filter(|d| !d.is_empty()),
        // 仅 AI 生成的元数据才记录模型名;本地快速索引为 None
        model: if !ai_used {
            None
        } else if vision.is_some() {
            Some(s.active_vision_model().to_string())
        } else {
            Some(s.active_text_model().to_string())
        },
        prompt_version: PROMPT_VERSION,
    };
    let chunk_ids = core.db.with_tx(|tx| {
        // 记录可能在处理期间被删除:此时外键会报错,视为无需处理
        repo::ensure_item_exists(tx, item_id)?;
        let ids = repo::replace_chunks(tx, item_id, &new_chunks)?;
        let pairs: Vec<(i64, i64)> = ids
            .iter()
            .zip(chunk_image_ids.iter())
            .flat_map(|(c, imgs)| imgs.iter().map(move |i| (*c, *i)))
            .collect();
        repo::add_chunk_images(tx, &pairs)?;
        repo::upsert_meta(tx, item_id, &new_meta)?;
        repo::set_item_status(tx, item_id, "done", None)?;
        Ok(ids)
    });
    let chunk_ids = match chunk_ids {
        Ok(v) => v,
        Err(AppError::Invalid(_)) => return Ok(()),
        Err(e) => return Err(e),
    };
    let entries: Vec<Entry> = chunk_ids
        .into_iter()
        .zip(vectors)
        .map(|(chunk_id, vec)| Entry { chunk_id, vec })
        .collect();
    // 紧贴写入前读取最新归属,避免处理期间被转移到别的知识库后写入旧归属
    let kb_id = core
        .db
        .with(|c| Ok(repo::get_item(c, item_id)?.map(|i| i.kb_id)))?
        .unwrap_or(crate::db::DEFAULT_KB_ID);
    core.vindex
        .replace_item_in(&embed_identity, item_id, kb_id, entries);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 采样保留首尾() {
        let text = (0..10000)
            .map(|i| char::from_u32(0x4e00 + (i % 500) as u32).unwrap())
            .collect::<String>();
        let s = sample_text(&text, 1000);
        assert!(s.chars().count() < 1100);
        assert!(s.starts_with(&text.chars().take(50).collect::<String>()));
        assert!(s.contains("……"));
        assert_eq!(sample_text("短文本", 1000), "短文本");
    }

    #[test]
    fn 清洗元数据() {
        let m = clean_meta(ExtractedMeta {
            summary: "  摘要  ".into(),
            category: "职场/项目动态非常长的分类名".into(),
            tags: vec!["#项目A".into(), "项目a".into(), "".into(), "预算".into()],
            entities: vec!["老王".into(), "老王".into()],
        });
        assert_eq!(m.summary, "摘要");
        assert_eq!(m.tags, vec!["项目A", "预算"]);
        assert_eq!(m.entities, vec!["老王"]);
        assert!(m.category.chars().count() <= 12);
    }

    #[test]
    fn 本地元数据只含摘要() {
        let m = local_meta("# 标题\n\n第一段内容。\n第二段");
        assert_eq!(m.summary, "标题 第一段内容。 第二段");
        assert!(m.tags.is_empty() && m.entities.is_empty());
        let long = "字".repeat(500);
        assert_eq!(local_meta(&long).summary.chars().count(), 120);
    }

    #[test]
    fn ai模式识别() {
        assert!(is_ai_mode("reindex_ai") && is_ai_mode("ingest_ai"));
        assert!(!is_ai_mode("reindex") && !is_ai_mode("ingest"));
    }

    #[test]
    fn 模型名匹配忽略latest() {
        let installed = vec!["bge-m3:latest".to_string(), "qwen2.5:7b".to_string()];
        assert!(model_installed(&installed, "bge-m3"));
        assert!(model_installed(&installed, "qwen2.5:7b"));
        assert!(!model_installed(&installed, "qwen2.5:3b"));
    }

    #[test]
    fn 向量化输入包含摘要与标题() {
        let meta = Some(ExtractedMeta {
            summary: "全文摘要".into(),
            tags: vec!["a".into()],
            ..Default::default()
        });
        let p = ChunkPiece {
            text: "正文".into(),
            heading: "一章".into(),
            page: None,
        };
        let d = build_embed_input("document", "文档名", &meta, &p);
        assert!(
            d.contains("《文档名》")
                && d.contains("摘要:全文摘要")
                && d.contains("[一章]")
                && d.contains("正文")
        );
        let n = build_embed_input("note", "", &meta, &p);
        assert!(n.contains("标签:a"));
    }
}
