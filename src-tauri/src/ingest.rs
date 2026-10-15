// @author ygw
//! 录入服务:闪念文字 / 图片 / 文档的落盘、去重与入队;记录的编辑、删除、重刷。
//!
//! 核心原则:**原始数据先落盘,AI 处理异步进行**。任何 AI 故障都不会导致数据丢失。

use crate::core::Core;
use crate::error::{AppError, Result};
use crate::extract::{self, FileKind};
use crate::folder;
use crate::queue;
use crate::repo::{self, NewItem};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 单条文字上限(超过请作为文档导入)
const MAX_NOTE_CHARS: usize = 200_000;
const MAX_IMAGE_BYTES: u64 = 30 * 1024 * 1024;
const MAX_DOC_BYTES: u64 = 200 * 1024 * 1024;
/// 防重复提交窗口
const DUP_WINDOW_MS: i64 = 10_000;

/// 闪念提交参数
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CaptureInput {
    /// 文字
    pub text: Option<String>,
    /// 文件路径(拖入的文件或剪贴板暂存文件)
    pub paths: Vec<String>,
    /// 导入到的知识库;None 为默认知识库
    pub kb_id: Option<i64>,
}

/// 闪念提交结果
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    /// 新建的记录 id
    pub created: Vec<i64>,
    /// 因重复被跳过的已有记录 id
    pub duplicates: Vec<i64>,
    /// 失败说明
    pub errors: Vec<String>,
    /// 被跳过的内容汇总(按原因合并,如"不支持的类型(.zip) ×3")
    pub skipped: Vec<String>,
    /// 被跳过的总数
    pub skipped_count: usize,
}

/// 已存储的附件信息
struct StoredFile {
    hash: String,
    rel_path: String,
    ext: String,
    original_name: String,
}

/// 待写入的记录
struct Prepared {
    item: NewItem,
}

/// 计算文件 SHA-256(流式)
fn hash_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// 把源文件按内容哈希存入附件目录(已存在则复用)
fn store_attachment(attach_root: &Path, src: &Path) -> Result<StoredFile> {
    let hash = hash_file(src)?;
    let ext = extract::ext_lower(src);
    let rel = format!("{}/{}.{}", &hash[..2], hash, ext);
    let dest = attach_root.join(&rel);
    if !dest.exists() {
        std::fs::create_dir_all(dest.parent().unwrap())?;
        // 先复制到临时名再重命名,避免中断留下半截文件
        let tmp = dest.with_extension(format!("{ext}.part"));
        std::fs::copy(src, &tmp)?;
        std::fs::rename(&tmp, &dest)?;
    }
    let original_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("未命名")
        .to_string();
    Ok(StoredFile {
        hash,
        rel_path: rel,
        ext,
        original_name,
    })
}

/// 解析附件相对路径为绝对路径,并防止目录穿越
///
/// 参数: core 核心;rel 数据库中的相对路径
/// 返回: 位于附件目录内的绝对路径
pub fn attachment_abs(core: &Core, rel: &str) -> Result<PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(AppError::Invalid("非法附件路径".into()));
    }
    Ok(core.paths.attachments.join(rel_path))
}

/// 暂存剪贴板/拖入的二进制内容到临时目录
///
/// 参数: core 核心;name 原始文件名;bytes 内容
/// 返回: 暂存文件的绝对路径
pub async fn stage_bytes(core: &Core, name: &str, bytes: Vec<u8>) -> Result<String> {
    if bytes.is_empty() {
        return Err(AppError::Invalid("内容为空".into()));
    }
    if bytes.len() as u64 > MAX_DOC_BYTES {
        return Err(AppError::Invalid("文件过大".into()));
    }
    // 仅保留文件名部分,避免路径注入
    let safe = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("paste.png");
    let safe: String = safe
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .collect();
    let dest = core
        .paths
        .tmp
        .join(format!("{}-{}", crate::db::now_ms(), safe));
    tokio::fs::write(&dest, bytes).await?;
    Ok(dest.to_string_lossy().into_owned())
}

/// 单个文件的处理结果
enum FileOutcome {
    /// 需要新建的记录
    New(Prepared),
    /// 知识库内已存在相同文件,给出已有记录 id
    Duplicate(i64),
}

/// 校验并存储单个文件,生成待写入的记录
///
/// 参数: core 核心;path 文件路径;label 用于错误提示的名称;kb_id 目标知识库;
/// source_path 来源路径(文件夹导入时的相对路径)
async fn prepare_file(
    core: &Arc<Core>,
    path: &Path,
    label: &str,
    kb_id: i64,
    source_path: Option<String>,
) -> Result<FileOutcome> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|_| AppError::Invalid(format!("文件不存在: {label}")))?;
    if !meta.is_file() {
        return Err(AppError::Invalid(format!("不是文件: {label}")));
    }
    let kind = extract::classify(path);
    let (limit, kind_name) = match kind {
        FileKind::Image => (MAX_IMAGE_BYTES, "image"),
        FileKind::Document => (MAX_DOC_BYTES, "document"),
        FileKind::Unsupported => {
            return Err(AppError::Invalid(format!(
                "不支持的文件类型: {}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or(label)
            )))
        }
    };
    if meta.len() == 0 {
        return Err(AppError::Invalid("文件为空".into()));
    }
    if meta.len() > limit {
        return Err(AppError::Invalid(format!(
            "文件过大(上限 {}MB)",
            limit / 1024 / 1024
        )));
    }
    let (ar, src) = (core.paths.attachments.clone(), path.to_path_buf());
    let stored = tokio::task::spawn_blocking(move || store_attachment(&ar, &src))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    // 暂存目录里的文件已复制进附件库,可以清理
    if path.starts_with(&core.paths.tmp) {
        let _ = tokio::fs::remove_file(path).await;
    }
    if let Some(id) = core
        .db
        .with(|c| repo::find_by_hash(c, &stored.hash, kind_name, kb_id))?
    {
        return Ok(FileOutcome::Duplicate(id));
    }
    let stem = Path::new(&stored.original_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let is_image = kind == FileKind::Image;
    Ok(FileOutcome::New(Prepared {
        item: NewItem {
            kind: kind_name.into(),
            title: if is_image { None } else { Some(stem) },
            content: String::new(),
            attachment_path: Some(stored.rel_path),
            original_name: Some(stored.original_name),
            mime: Some(extract::mime_of(&stored.ext).into()),
            content_hash: Some(stored.hash),
            kb_id: Some(kb_id),
            source_path,
        },
    }))
}

/// 处理闪念提交:文字 → 笔记;图片 → 图片记录(文字作为说明);文档 → 文档记录
///
/// 参数: core 核心;input 提交内容
/// 返回: 创建/重复/失败汇总
pub async fn capture(core: &Arc<Core>, input: CaptureInput) -> Result<CaptureResult> {
    let text = input
        .text
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if text.is_empty() && input.paths.is_empty() {
        return Err(AppError::Invalid("内容为空".into()));
    }
    if text.chars().count() > MAX_NOTE_CHARS {
        return Err(AppError::Invalid(
            "文字过长,请保存为文件后作为文档导入".into(),
        ));
    }
    let kb_id = input.kb_id.unwrap_or(crate::db::DEFAULT_KB_ID);
    core.db.with(|c| repo::ensure_kb_exists(c, kb_id))?;
    let mut result = CaptureResult::default();
    let mut prepared: Vec<Prepared> = Vec::new();
    let mut has_image = false;
    let mut has_doc = false;

    // 本次导入已见过的 (内容哈希, 类型):同一批里内容相同的文件只导入一份
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut skip_map: BTreeMap<String, usize> = BTreeMap::new();

    for p in &input.paths {
        let path = PathBuf::from(p);
        let is_dir = tokio::fs::metadata(&path)
            .await
            .map(|m| m.is_dir())
            .unwrap_or(false);
        // 目录展开为其中受支持的文件;单个文件直接处理
        let targets: Vec<(PathBuf, Option<String>)> = if is_dir {
            let root = path.clone();
            let scan = tokio::task::spawn_blocking(move || folder::scan(&root))
                .await
                .map_err(|e| AppError::Other(e.to_string()))?;
            for (reason, n) in &scan.skipped {
                *skip_map.entry(reason.clone()).or_insert(0) += n;
            }
            scan.files
                .into_iter()
                .map(|f| (f.path, Some(f.rel)))
                .collect()
        } else {
            vec![(path, None)]
        };
        let from_folder = is_dir;
        for (path, rel) in targets {
            let label = path.to_string_lossy().into_owned();
            match prepare_file(core, &path, &label, kb_id, rel).await {
                Ok(FileOutcome::New(pr)) => {
                    let key = (
                        pr.item.content_hash.clone().unwrap_or_default(),
                        pr.item.kind.clone(),
                    );
                    if !seen.insert(key) {
                        *skip_map
                            .entry("与本次导入的另一文件内容相同".to_string())
                            .or_insert(0) += 1;
                        continue;
                    }
                    has_image |= pr.item.kind == "image";
                    has_doc |= pr.item.kind == "document";
                    prepared.push(pr);
                }
                Ok(FileOutcome::Duplicate(id)) => result.duplicates.push(id),
                // 文件夹里的个别坏文件不算失败,归入"跳过"并给出原因
                Err(e) if from_folder => {
                    *skip_map.entry(e.to_string()).or_insert(0) += 1;
                }
                Err(e) => result.errors.push(e.to_string()),
            }
        }
    }
    result.skipped_count = skip_map.values().sum();
    result.skipped = folder::skip_lines(&skip_map);

    // 文字的归属:有图片则作为图片说明;否则(或同时有文档)独立成笔记
    if !text.is_empty() {
        if has_image {
            for pr in prepared.iter_mut().filter(|p| p.item.kind == "image") {
                pr.item.content = text.clone();
            }
        }
        if !has_image || has_doc {
            if let Some(id) = core
                .db
                .with(|c| repo::find_recent_duplicate_note(c, &text, DUP_WINDOW_MS, kb_id))?
            {
                result.duplicates.push(id);
            } else {
                prepared.push(Prepared {
                    item: NewItem {
                        kind: "note".into(),
                        content: text.clone(),
                        kb_id: Some(kb_id),
                        ..Default::default()
                    },
                });
            }
        }
    }

    if prepared.is_empty() && result.duplicates.is_empty() && !result.errors.is_empty() {
        return Err(AppError::Invalid(result.errors.join(";")));
    }

    // 单事务写入记录并入队
    let ids = core.db.with_tx(|tx| {
        let mut ids = Vec::new();
        for pr in &prepared {
            let id = repo::insert_item(tx, &pr.item)?;
            queue::enqueue_item(tx, id, "ingest")?;
            ids.push(id);
        }
        Ok(ids)
    })?;
    result.created = ids;
    core.wake.notify_one();
    for id in &result.created {
        core.emit("item-updated", json!({ "id": id }));
    }
    queue::emit_status(core);
    Ok(result)
}

/// 编辑记录原文并触发重新解析
///
/// 参数: core 核心;id 记录 id;content 新原文
pub fn update_content(core: &Arc<Core>, id: i64, content: &str) -> Result<()> {
    let content = content.trim();
    if content.chars().count() > MAX_NOTE_CHARS * 10 {
        return Err(AppError::Invalid("文字过长".into()));
    }
    let item = core
        .db
        .with(|c| repo::get_item(c, id))?
        .ok_or_else(|| AppError::Invalid("记录不存在".into()))?;
    if item.kind == "note" && content.is_empty() {
        return Err(AppError::Invalid("笔记内容不能为空".into()));
    }
    if item.kind == "document" && content.is_empty() {
        return Err(AppError::Invalid("文档内容不能为空".into()));
    }
    core.db.with_tx(|tx| {
        repo::update_item_text(tx, id, None, content)?;
        queue::enqueue_item(tx, id, "reindex")?;
        Ok(())
    })?;
    core.wake.notify_one();
    core.emit("item-updated", json!({ "id": id }));
    queue::emit_status(core);
    Ok(())
}

/// 删除记录(含向量、FTS、无人引用的附件文件)
///
/// 参数: core 核心;ids 记录 id 列表
/// 返回: 实际删除的条数
pub async fn delete_items(core: &Arc<Core>, ids: &[i64]) -> Result<usize> {
    let mut n = 0;
    for &id in ids {
        queue::cancel_item(core, id)?;
        let attachment = core
            .db
            .with(|c| Ok(repo::get_item(c, id)?.and_then(|i| i.attachment_path)))?;
        let deleted = core.db.with_tx(|tx| {
            let exists: i64 =
                tx.query_row("SELECT count(*) FROM items WHERE id=?1", [id], |r| r.get(0))?;
            if exists == 0 {
                return Ok(false);
            }
            tx.execute("DELETE FROM jobs WHERE item_id = ?1", [id])?;
            repo::delete_item(tx, id)?;
            Ok(true)
        })?;
        if !deleted {
            continue;
        }
        n += 1;
        core.vindex.remove_item(id);
        if let Some(rel) = attachment {
            if core.db.with(|c| repo::count_attachment_refs(c, &rel))? == 0 {
                if let Ok(abs) = attachment_abs(core, &rel) {
                    let _ = tokio::fs::remove_file(abs).await;
                }
            }
        }
        core.emit("item-deleted", json!({ "id": id }));
    }
    queue::emit_status(core);
    Ok(n)
}

/// 重刷:选中记录或全库(文档走快速索引,除非设置开启 AI)
///
/// 参数: core 核心;ids 指定记录(None 表示全库)
/// 返回: 入队数量
pub fn reindex(core: &Arc<Core>, ids: Option<Vec<i64>>) -> Result<usize> {
    reindex_with_mode(core, ids, "reindex")
}

/// AI 解读:对选中记录强制调用模型生成摘要/标签/实体
///
/// 参数: core 核心;ids 指定记录(不允许为空)
/// 返回: 入队数量
pub fn reindex_ai(core: &Arc<Core>, ids: Vec<i64>) -> Result<usize> {
    if ids.is_empty() {
        return Err(AppError::Invalid("未选择记录".into()));
    }
    reindex_with_mode(core, Some(ids), "reindex_ai")
}

/// 按指定模式把记录加入重刷队列
fn reindex_with_mode(core: &Arc<Core>, ids: Option<Vec<i64>>, mode: &str) -> Result<usize> {
    let n = core.db.with_tx(|tx| {
        let targets: Vec<i64> = match ids {
            Some(v) => v,
            None => {
                let mut stmt = tx.prepare("SELECT id FROM items ORDER BY id")?;
                let rows = stmt.query_map([], |r| r.get(0))?;
                rows.collect::<std::result::Result<Vec<i64>, _>>()?
            }
        };
        let mut n = 0;
        for id in targets {
            repo::ensure_item_exists(tx, id)?;
            if queue::enqueue_item(tx, id, mode)? {
                n += 1;
            }
        }
        Ok(n)
    })?;
    core.wake.notify_one();
    queue::emit_status(core);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_core;

    #[tokio::test]
    async fn 空内容被拒绝() {
        let dir = tempfile::tempdir().unwrap();
        let core = test_core(dir.path(), "http://127.0.0.1:9");
        assert!(capture(&core, CaptureInput::default()).await.is_err());
        assert!(capture(
            &core,
            CaptureInput {
                text: Some("  ".into()),
                paths: vec![],
                ..Default::default()
            }
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn 文字先落盘并入队() {
        let dir = tempfile::tempdir().unwrap();
        let core = test_core(dir.path(), "http://127.0.0.1:9");
        let r = capture(
            &core,
            CaptureInput {
                text: Some("今天和老王聊了A项目".into()),
                paths: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(r.created.len(), 1);
        let it = core
            .db
            .with(|c| repo::get_item(c, r.created[0]))
            .unwrap()
            .unwrap();
        assert_eq!(it.status, "pending");
        let jobs: i64 = core
            .db
            .with(|c| Ok(c.query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(jobs, 1);
    }

    #[tokio::test]
    async fn 短时间重复提交被去重() {
        let dir = tempfile::tempdir().unwrap();
        let core = test_core(dir.path(), "http://127.0.0.1:9");
        let a = capture(
            &core,
            CaptureInput {
                text: Some("重复".into()),
                paths: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let b = capture(
            &core,
            CaptureInput {
                text: Some("重复".into()),
                paths: vec![],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(a.created.len(), 1);
        assert!(b.created.is_empty());
        assert_eq!(b.duplicates, a.created);
    }

    #[tokio::test]
    async fn 图片带说明且按哈希去重() {
        let dir = tempfile::tempdir().unwrap();
        let core = test_core(dir.path(), "http://127.0.0.1:9");
        let img = dir.path().join("a.png");
        std::fs::write(&img, b"\x89PNG fake image bytes").unwrap();
        let r = capture(
            &core,
            CaptureInput {
                text: Some("Redis 架构图".into()),
                paths: vec![img.to_string_lossy().into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(r.created.len(), 1);
        let it = core
            .db
            .with(|c| repo::get_item(c, r.created[0]))
            .unwrap()
            .unwrap();
        assert_eq!(it.kind, "image");
        assert_eq!(it.content, "Redis 架构图");
        let rel = it.attachment_path.unwrap();
        assert!(core.paths.attachments.join(&rel).exists());
        let r2 = capture(
            &core,
            CaptureInput {
                text: None,
                paths: vec![img.to_string_lossy().into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(r2.duplicates.len(), 1);
        assert!(r2.created.is_empty());
    }

    #[tokio::test]
    async fn 不支持的文件报错且不影响文字() {
        let dir = tempfile::tempdir().unwrap();
        let core = test_core(dir.path(), "http://127.0.0.1:9");
        let bad = dir.path().join("a.exe");
        std::fs::write(&bad, b"x").unwrap();
        let r = capture(
            &core,
            CaptureInput {
                text: Some("还是要存".into()),
                paths: vec![bad.to_string_lossy().into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(r.created.len(), 1);
        assert_eq!(r.errors.len(), 1);
    }

    #[tokio::test]
    async fn 删除记录会清理附件() {
        let dir = tempfile::tempdir().unwrap();
        let core = test_core(dir.path(), "http://127.0.0.1:9");
        let doc = dir.path().join("a.md");
        std::fs::write(&doc, "# 标题\n内容").unwrap();
        let r = capture(
            &core,
            CaptureInput {
                text: None,
                paths: vec![doc.to_string_lossy().into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let rel = core
            .db
            .with(|c| repo::get_item(c, r.created[0]))
            .unwrap()
            .unwrap()
            .attachment_path
            .unwrap();
        let abs = core.paths.attachments.join(&rel);
        assert!(abs.exists());
        assert_eq!(delete_items(&core, &r.created).await.unwrap(), 1);
        assert!(!abs.exists());
    }

    #[test]
    fn 附件路径防穿越() {
        let dir = tempfile::tempdir().unwrap();
        let core = test_core(dir.path(), "http://127.0.0.1:9");
        assert!(attachment_abs(&core, "../etc/passwd").is_err());
        assert!(attachment_abs(&core, "/etc/passwd").is_err());
        assert!(attachment_abs(&core, "ab/cd.png").is_ok());
    }
}
