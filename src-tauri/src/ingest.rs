// @author ixxxxoooo
//! 录入服务:闪念文字 / 图片 / 文档的落盘、去重与入队;记录的编辑、删除、重刷。
//!
//! 核心原则:**原始数据先落盘,AI 处理异步进行**。任何 AI 故障都不会导致数据丢失。

use crate::core::Core;
use crate::error::{AppError, Result};
use crate::extract::{self, FileKind};
use crate::folder;
use crate::formats;
use crate::images;
use crate::queue;
use crate::repo::{self, NewItem};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
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
    /// created 中由文档引用/内嵌自动导入并关联的图片数
    pub linked_images: usize,
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
    /// 该文档引用的图片
    links: Vec<PendingLink>,
}

/// 文档里的一处图片引用(写入前,子记录可能尚未入库)
struct PendingLink {
    child: ChildRef,
    /// 原文中的引用写法
    anchor: String,
}

/// 关联的子记录:库内已有,或本批次待写入的第几条
#[derive(Debug, Clone, Copy)]
enum ChildRef {
    Existing(i64),
    Pending(usize),
}

/// 一次导入的累积状态
#[derive(Default)]
struct Batch {
    prepared: Vec<Prepared>,
    /// 本次导入已见过的 (内容哈希, 类型) → 对应的待写入记录,同一批里内容相同的文件只导入一份
    seen: HashMap<(String, String), ChildRef>,
    /// 被跳过内容的原因统计
    skip_map: BTreeMap<String, usize>,
    has_image: bool,
    has_doc: bool,
}

impl Batch {
    fn skip(&mut self, reason: impl Into<String>) {
        *self.skip_map.entry(reason.into()).or_insert(0) += 1;
    }
}

/// 登记图片的结果
enum Registered {
    /// 新建了待写入的图片记录
    Created(ChildRef),
    /// 知识库里已有同内容图片
    Existing(i64),
    /// 本批次已登记过同内容图片
    SeenInBatch(ChildRef),
}

impl Registered {
    /// 对应的子记录
    fn child(&self) -> ChildRef {
        match *self {
            Registered::Created(c) | Registered::SeenInBatch(c) => c,
            Registered::Existing(id) => ChildRef::Existing(id),
        }
    }
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
    // Windows 上 "/etc/passwd"、"C:foo" 不算 is_absolute,但 join 后仍会逃出附件目录,
    // 因此按路径组件拒绝根目录、盘符前缀与 ".."
    if rel_path.is_absolute()
        || rel_path.has_root()
        || rel_path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
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
    New(Box<Prepared>),
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
    embedded: bool,
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
    Ok(FileOutcome::New(Box::new(Prepared {
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
            embedded,
        },
        links: vec![],
    })))
}

/// 登记一张图片:库内已有则复用,本批次已有则合并,否则加入待写入列表
///
/// 参数: embedded 为 true 表示由文档引用自动导入(最后一个引用它的文档被删除时一并清理)
async fn register_image(
    core: &Arc<Core>,
    batch: &mut Batch,
    path: &Path,
    kb_id: i64,
    embedded: bool,
) -> Result<Registered> {
    let label = path.to_string_lossy().into_owned();
    match prepare_file(core, path, &label, kb_id, None, embedded).await? {
        FileOutcome::Duplicate(id) => {
            // 用户显式导入了此前自动导入的图片:转为普通图片
            if !embedded {
                core.db.with(|c| repo::promote_embedded(c, id))?;
            }
            Ok(Registered::Existing(id))
        }
        FileOutcome::New(pr) => {
            let key = (
                pr.item.content_hash.clone().unwrap_or_default(),
                "image".to_string(),
            );
            if let Some(slot) = batch.seen.get(&key).copied() {
                // 显式导入时,原先自动导入的图片转为普通图片
                if !embedded {
                    if let ChildRef::Pending(i) = slot {
                        batch.prepared[i].item.embedded = false;
                    }
                }
                return Ok(Registered::SeenInBatch(slot));
            }
            let idx = batch.prepared.len();
            batch.prepared.push(*pr);
            batch.seen.insert(key, ChildRef::Pending(idx));
            batch.has_image = true;
            Ok(Registered::Created(ChildRef::Pending(idx)))
        }
    }
}

/// 收集文档引用/内嵌的图片并登记为待写入记录
///
/// - md / html:解析 `![](相对路径)` 与 `<img src>`,相对文档所在目录解析(此时原始路径还在,
///   入库后附件按哈希重命名,相对路径信息会丢失,所以必须在导入时完成)
/// - docx:按正文出现顺序读取 `word/media` 内嵌图片
///
/// 返回: 文档到图片的关联(按出现顺序、同一引用写法只保留一条)
async fn collect_doc_images(
    core: &Arc<Core>,
    path: &Path,
    kb_id: i64,
    batch: &mut Batch,
) -> Vec<PendingLink> {
    let ext = extract::ext_lower(path);
    let mut links: Vec<PendingLink> = Vec::new();
    match ext.as_str() {
        "md" | "markdown" | "html" | "htm" => {
            let Ok(bytes) = tokio::fs::read(path).await else {
                return links;
            };
            // 过大的文本不扫描引用(上限 20MB)
            if bytes.len() > 20 * 1024 * 1024 {
                return links;
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let mut anchors: HashSet<String> = HashSet::new();
            for r in images::scan_refs(&text) {
                if images::is_remote(&r.src) || !anchors.insert(r.src.clone()) {
                    continue;
                }
                if links.len() >= images::MAX_IMAGES_PER_DOC {
                    batch.skip(format!(
                        "文档引用图片过多,仅导入前 {} 张",
                        images::MAX_IMAGES_PER_DOC
                    ));
                    break;
                }
                let Some(img) = images::resolve_local(&base, &r.src) else {
                    batch.skip("引用的图片找不到");
                    continue;
                };
                match register_image(core, batch, &img, kb_id, true).await {
                    Ok(reg) => links.push(PendingLink {
                        child: reg.child(),
                        anchor: r.src.clone(),
                    }),
                    Err(e) => batch.skip(e.to_string()),
                }
            }
        }
        "docx" | "pptx" | "epub" => {
            let p = path.to_path_buf();
            let kind = ext.clone();
            let Ok(Ok((imgs, skipped))) =
                tokio::task::spawn_blocking(move || match kind.as_str() {
                    "pptx" => formats::pptx_images(&p),
                    "epub" => formats::epub_images(&p),
                    _ => images::docx_images(&p),
                })
                .await
            else {
                return links;
            };
            for s in skipped {
                batch.skip(s);
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("文档")
                .to_string();
            for img in imgs {
                // 写到暂存目录再走统一的校验/入库流程(入库后暂存文件会被清理)
                let dir =
                    core.paths
                        .tmp
                        .join(format!("emb-{}-{}", crate::db::now_ms(), links.len()));
                if tokio::fs::create_dir_all(&dir).await.is_err() {
                    continue;
                }
                let file = dir.join(format!("{stem}_{}", img.name));
                let wrote = tokio::fs::write(&file, &img.bytes).await.is_ok();
                if wrote {
                    match register_image(core, batch, &file, kb_id, true).await {
                        Ok(reg) => links.push(PendingLink {
                            child: reg.child(),
                            anchor: img.anchor.clone(),
                        }),
                        Err(e) => batch.skip(e.to_string()),
                    }
                }
                let _ = tokio::fs::remove_dir_all(&dir).await;
            }
        }
        _ => {}
    }
    links
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
    let mut batch = Batch::default();

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
                *batch.skip_map.entry(reason.clone()).or_insert(0) += n;
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
            let is_image = extract::classify(&path) == FileKind::Image;
            let outcome: Result<()> = async {
                if is_image {
                    // 显式导入的图片与文档引用的图片共用登记逻辑(库内/批内按哈希去重)
                    let meta = tokio::fs::metadata(&path)
                        .await
                        .map_err(|_| AppError::Invalid(format!("文件不存在: {label}")))?;
                    if !meta.is_file() {
                        return Err(AppError::Invalid(format!("不是文件: {label}")));
                    }
                    match prepare_file(core, &path, &label, kb_id, rel.clone(), false).await? {
                        FileOutcome::Duplicate(id) => {
                            core.db.with(|c| repo::promote_embedded(c, id))?;
                            result.duplicates.push(id);
                        }
                        FileOutcome::New(mut pr) => {
                            let key = (
                                pr.item.content_hash.clone().unwrap_or_default(),
                                "image".to_string(),
                            );
                            match batch.seen.get(&key).copied() {
                                Some(ChildRef::Pending(i)) => {
                                    // 先被文档引用登记过:转为普通图片并补上来源路径
                                    if batch.prepared[i].item.embedded {
                                        batch.prepared[i].item.embedded = false;
                                        batch.prepared[i].item.source_path = rel.clone();
                                    } else {
                                        batch.skip("与本次导入的另一文件内容相同");
                                    }
                                }
                                Some(ChildRef::Existing(_)) => {
                                    batch.skip("与本次导入的另一文件内容相同")
                                }
                                None => {
                                    pr.item.embedded = false;
                                    let idx = batch.prepared.len();
                                    batch.seen.insert(key, ChildRef::Pending(idx));
                                    batch.has_image = true;
                                    batch.prepared.push(*pr);
                                }
                            }
                        }
                    }
                    return Ok(());
                }
                match prepare_file(core, &path, &label, kb_id, rel.clone(), false).await? {
                    FileOutcome::New(mut pr) => {
                        let key = (
                            pr.item.content_hash.clone().unwrap_or_default(),
                            pr.item.kind.clone(),
                        );
                        if batch.seen.contains_key(&key) {
                            batch.skip("与本次导入的另一文件内容相同");
                            return Ok(());
                        }
                        // 先收集文档引用的图片(子记录排在文档之前入库)
                        pr.links = collect_doc_images(core, &path, kb_id, &mut batch).await;
                        batch
                            .seen
                            .insert(key, ChildRef::Pending(batch.prepared.len()));
                        batch.has_doc |= pr.item.kind == "document";
                        batch.prepared.push(*pr);
                    }
                    FileOutcome::Duplicate(id) => result.duplicates.push(id),
                }
                Ok(())
            }
            .await;
            match outcome {
                Ok(()) => {}
                // 文件夹里的个别坏文件不算失败,归入"跳过"并给出原因
                Err(e) if from_folder => batch.skip(e.to_string()),
                Err(e) => result.errors.push(e.to_string()),
            }
        }
    }
    result.skipped_count = batch.skip_map.values().sum();
    result.skipped = folder::skip_lines(&batch.skip_map);
    let (has_image, has_doc) = (batch.has_image, batch.has_doc);
    let mut prepared = std::mem::take(&mut batch.prepared);

    // 文字的归属:有图片则作为图片说明;否则(或同时有文档)独立成笔记
    if !text.is_empty() {
        if has_image {
            // 文字作为说明只附在显式导入的图片上(文档里自动导入的图片不加)
            for pr in prepared
                .iter_mut()
                .filter(|p| p.item.kind == "image" && !p.item.embedded)
            {
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
                    links: vec![],
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
        // 文档 → 图片的关联(子记录一定排在文档之前,此时 id 已确定)
        for (i, pr) in prepared.iter().enumerate() {
            for (seq, l) in pr.links.iter().enumerate() {
                let child = match l.child {
                    ChildRef::Existing(id) => id,
                    ChildRef::Pending(idx) => ids[idx],
                };
                repo::add_link(tx, ids[i], child, "image", &l.anchor, seq as i64)?;
            }
        }
        Ok(ids)
    })?;
    result.linked_images = prepared.iter().filter(|p| p.item.embedded).count();
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
    // 待删除队列:删除文档后,只被它引用的内嵌图片会成为孤儿,一并追加到队列
    let mut todo: std::collections::VecDeque<i64> = ids.iter().copied().collect();
    let mut done: HashSet<i64> = HashSet::new();
    while let Some(id) = todo.pop_front() {
        if !done.insert(id) {
            continue;
        }
        queue::cancel_item(core, id)?;
        let attachment = core
            .db
            .with(|c| Ok(repo::get_item(c, id)?.and_then(|i| i.attachment_path)))?;
        let children = core.db.with(|c| repo::linked_children(c, &[id]))?;
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
        // 关联已随外键级联删除:挑出"内嵌导入且不再被任何文档引用"的图片
        if !children.is_empty() {
            for orphan in core.db.with(|c| repo::orphan_embedded(c, &children))? {
                todo.push_back(orphan);
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
        #[cfg(windows)]
        {
            assert!(attachment_abs(&core, "C:\\Windows\\win.ini").is_err());
            assert!(attachment_abs(&core, "C:win.ini").is_err());
        }
        assert!(attachment_abs(&core, "ab/cd.png").is_ok());
    }
}
