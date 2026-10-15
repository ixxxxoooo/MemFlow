// @author ixxxxoooo
//! PDF 入库解析:逐页读取文本层,扫描页走本机 OCR(PDFKit + Vision),
//! 或按设置交给视觉模型逐页转 Markdown。输出带页边界标记(`chunker::PAGE_MARK`)的文本。
//!
//! 任一环节不可用时回退到 `pdf_extract` 的纯文本抽取,保证旧行为不退化。

use crate::chunker::PAGE_MARK;
use crate::core::{permit, CancelToken, Core};
use crate::error::{AppError, Result};
use crate::extract;
use crate::layout;
use crate::pdf_native::{self, PdfDoc};
use crate::pipeline::model_installed;
use crate::prompts;
use crate::settings::PDF_MODE_VISION;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 单个 PDF 最多处理的页数
const MAX_PAGES: usize = 2000;
/// 单页 OCR / 视觉解析超时
const PAGE_TIMEOUT: Duration = Duration::from_secs(90);
/// 视觉模型单页超时(本地模型更慢)
const VISION_PAGE_TIMEOUT: Duration = Duration::from_secs(240);
/// 整个 PDF 的解析时间预算,超出后剩余页只读文本层
const TOTAL_BUDGET: Duration = Duration::from_secs(30 * 60);
/// 发给视觉模型的页面图长边像素
const VISION_EDGE: f64 = 1800.0;

/// 页面解析结果的来源统计
#[derive(Debug, Default, Clone, Copy)]
struct PageStats {
    text_pages: usize,
    ocr_pages: usize,
    vision_pages: usize,
    skipped_pages: usize,
}

/// 把每页的 Markdown 拼成带页标记的全文
///
/// 参数: pages 按页序排列的页内容(空页也要保留,页码由标记出现次数推出)
/// 返回: 每页以 `PAGE_MARK` 单独成行开头的文本
pub fn join_pages(pages: &[String]) -> String {
    let mut out = String::new();
    for md in pages {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push(PAGE_MARK);
        out.push('\n');
        out.push_str(md.trim());
    }
    out
}

/// 解析 PDF
///
/// 参数: core 应用核心;path PDF 路径;token 取消令牌
/// 返回: 清洗后的带页标记文本
pub async fn extract_pdf(core: &Core, path: &Path, token: &CancelToken) -> Result<String> {
    let mode = core.settings().pdf_parse_mode;
    if !pdf_native::AVAILABLE {
        return extract::extract_text(path).await;
    }
    let p = path.to_path_buf();
    let doc = tokio::task::spawn_blocking(move || PdfDoc::open(&p))
        .await
        .map_err(|e| AppError::Parse(format!("PDF 打开任务失败: {e}")))?;
    let Some(doc) = doc else {
        tracing::warn!("原生 PDF 打开失败,回退纯文本抽取: {}", path.display());
        return extract::extract_text(path).await;
    };
    let total = doc.page_count.min(MAX_PAGES);
    if doc.page_count > MAX_PAGES {
        tracing::warn!("PDF 共 {} 页,只处理前 {MAX_PAGES} 页", doc.page_count);
    }
    let doc = Arc::new(Mutex::new(doc));
    let vision = mode == PDF_MODE_VISION;
    if vision {
        ensure_vision_model(core).await?;
    }

    let started = Instant::now();
    let mut stats = PageStats::default();
    let mut pages: Vec<String> = Vec::with_capacity(total);
    for i in 0..total {
        if token.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let over_budget = started.elapsed() > TOTAL_BUDGET;
        let md = if vision && !over_budget {
            match vision_page(core, &doc, i).await {
                Ok(md) if !md.trim().is_empty() => {
                    stats.vision_pages += 1;
                    md
                }
                other => {
                    if let Err(e) = other {
                        tracing::warn!("第 {} 页视觉解析失败,改用本机解析: {e}", i + 1);
                    }
                    local_page(core, &doc, i, true, &mut stats).await?
                }
            }
        } else {
            local_page(core, &doc, i, !over_budget, &mut stats).await?
        };
        pages.push(md);
        if (i + 1) % 20 == 0 {
            tracing::info!("PDF 解析进度 {}/{total}", i + 1);
        }
    }
    drop(doc);
    tracing::info!(
        "PDF 解析完成: {total} 页(文本层 {} / OCR {} / 视觉 {} / 跳过 {}),耗时 {:?}",
        stats.text_pages,
        stats.ocr_pages,
        stats.vision_pages,
        stats.skipped_pages,
        started.elapsed()
    );
    let text = extract::clean_text(&join_pages(&pages));
    if text.trim().is_empty() {
        return Err(AppError::Parse(
            "未能从 PDF 中识别到文字(可能是空白页或图片过于模糊)".into(),
        ));
    }
    Ok(truncate_marked(text, extract::MAX_TEXT_CHARS))
}

/// 保留页标记的截断:按字符数截断,避免尾部出现半截标记
fn truncate_marked(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        s
    } else {
        s.chars().take(max).collect()
    }
}

/// 视觉模式前置检查:本地视觉模型必须已安装;云端视觉模型无需安装
async fn ensure_vision_model(core: &Core) -> Result<()> {
    let s = core.settings();
    if s.uses_cloud_vision() {
        return Ok(());
    }
    let installed: Vec<String> = core
        .ollama()
        .list_models()
        .await?
        .into_iter()
        .map(|m| m.name)
        .collect();
    if !model_installed(&installed, &s.vision_model) {
        return Err(AppError::ModelMissing(s.vision_model));
    }
    Ok(())
}

/// 本机解析一页:有文本层则重排,否则(允许时)OCR
async fn local_page(
    core: &Core,
    doc: &Arc<Mutex<PdfDoc>>,
    idx: usize,
    allow_ocr: bool,
    stats: &mut PageStats,
) -> Result<String> {
    let _cpu = permit(&core.stages.cpu).await?;
    let d = doc.clone();
    let job = tokio::task::spawn_blocking(move || -> Result<(String, bool)> {
        let guard = d
            .lock()
            .map_err(|_| AppError::Parse("PDF 句柄异常".into()))?;
        let text = guard.page_text(idx).unwrap_or_default();
        if !layout::is_scanned(&text) {
            return Ok((layout::reflow_text_layer(&text), false));
        }
        if !allow_ocr {
            return Ok((String::new(), false));
        }
        let lines = guard.page_ocr_lines(idx)?;
        Ok((layout::lines_to_markdown(&lines), true))
    });
    let (md, ocr) = match tokio::time::timeout(PAGE_TIMEOUT, job).await {
        Err(_) => {
            tracing::warn!("第 {} 页解析超时,已跳过", idx + 1);
            stats.skipped_pages += 1;
            return Ok(String::new());
        }
        Ok(Err(e)) => return Err(AppError::Parse(format!("PDF 页面任务异常: {e}"))),
        Ok(Ok(Err(e))) => {
            tracing::warn!("第 {} 页解析失败,已跳过: {e}", idx + 1);
            stats.skipped_pages += 1;
            return Ok(String::new());
        }
        Ok(Ok(Ok(v))) => v,
    };
    if md.trim().is_empty() {
        stats.skipped_pages += 1;
    } else if ocr {
        stats.ocr_pages += 1;
    } else {
        stats.text_pages += 1;
    }
    Ok(md)
}

/// 视觉模型解析一页:渲染成图片 → 视觉模型转 Markdown
async fn vision_page(core: &Core, doc: &Arc<Mutex<PdfDoc>>, idx: usize) -> Result<String> {
    let d = doc.clone();
    let png = {
        let _cpu = permit(&core.stages.cpu).await?;
        tokio::task::spawn_blocking(move || {
            d.lock().ok().and_then(|g| g.page_png(idx, VISION_EDGE))
        })
        .await
        .map_err(|e| AppError::Parse(format!("页面渲染任务异常: {e}")))?
    }
    .ok_or_else(|| AppError::Parse("页面渲染失败".into()))?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let (client, model) = core.vision_backend()?;
    // 本地视觉模型与本地文本模型共用串行许可,避免同时占用内存;云端只受云端并发上限约束
    let _llm = if client.is_cloud() {
        permit(&core.stages.llm_cloud).await?
    } else {
        let p = permit(&core.stages.llm_local).await?;
        core.prepare_chat_model(&model).await;
        p
    };
    let msgs = prompts::pdf_page_messages(b64);
    let text = tokio::time::timeout(
        VISION_PAGE_TIMEOUT,
        client.chat(&model, &msgs, &core.gen_opts(0.0), None),
    )
    .await
    .map_err(|_| AppError::Parse("视觉模型解析超时".into()))??;
    Ok(crate::ollama::strip_code_fence(&text).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 拼接页内容并保留空页占位() {
        let t = join_pages(&["甲".into(), "  ".into(), "丙".into()]);
        assert_eq!(
            t,
            format!("{PAGE_MARK}\n甲\n\n{PAGE_MARK}\n\n\n{PAGE_MARK}\n丙")
        );
    }
}
