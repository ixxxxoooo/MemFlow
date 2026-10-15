// @author ygw
//! 数据访问层:记录、元数据、切片、设置、对话的 SQL 封装。

use crate::db::{now_ms, DEFAULT_KB_ID};
use crate::error::{AppError, Result};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

/// 记录视图(返回给前端,camelCase)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: i64,
    pub kind: String,
    /// 所属知识库
    pub kb_id: i64,
    pub title: Option<String>,
    /// 原文(列表场景为前 600 字的预览)
    pub content: String,
    /// 原文总字符数
    pub content_length: i64,
    pub attachment_path: Option<String>,
    pub original_name: Option<String>,
    pub mime: Option<String>,
    /// 文件夹导入时的来源路径(文件夹名/相对路径)
    pub source_path: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub summary: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub entities: Vec<String>,
    pub vision_text: Option<String>,
    /// 本机 OCR 识别出的图中文字(图片记录)
    pub ocr_text: Option<String>,
    /// 是否由文档内嵌/引用自动导入的图片
    pub embedded: bool,
    pub meta_model: Option<String>,
    pub prompt_version: Option<i64>,
    pub chunk_count: i64,
}

/// 切片视图
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkView {
    pub id: i64,
    pub seq: i64,
    pub text: String,
    pub heading: Option<String>,
    pub doc_summary: Option<String>,
    pub embed_model: Option<String>,
    /// 所在页码(PDF 文档)
    pub page: Option<i64>,
    /// 该切片里出现的图片
    pub images: Vec<ImageRef>,
}

/// 切片关联的图片(用于展示缩略图)
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    pub id: i64,
    pub attachment_path: Option<String>,
    pub name: Option<String>,
}

/// 记录列表过滤条件
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemFilter {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub tag: Option<String>,
    pub search: Option<String>,
    pub before_id: Option<i64>,
    pub limit: Option<i64>,
    /// 限定知识库(空 = 全部)
    pub kb_id: Option<i64>,
}

/// 新建记录参数
#[derive(Debug, Clone, Default)]
pub struct NewItem {
    pub kind: String,
    pub title: Option<String>,
    pub content: String,
    pub attachment_path: Option<String>,
    pub original_name: Option<String>,
    pub mime: Option<String>,
    pub content_hash: Option<String>,
    /// 所属知识库;None 表示默认知识库
    pub kb_id: Option<i64>,
    /// 来源路径(文件夹导入)
    pub source_path: Option<String>,
    /// 是否由文档内嵌/引用自动导入的图片
    pub embedded: bool,
}

/// 新建切片参数
#[derive(Debug, Clone)]
pub struct NewChunk {
    pub text: String,
    pub heading: String,
    pub doc_summary: Option<String>,
    pub embed_model: String,
    pub embedding: Vec<u8>,
    pub dim: usize,
    /// 所在页码(PDF 文档)
    pub page: Option<i64>,
    /// 写入 FTS5 的分词串
    pub fts_tokens: String,
}

/// 派生元数据
#[derive(Debug, Clone, Default)]
pub struct NewMeta {
    pub summary: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub entities: Vec<String>,
    pub vision_text: Option<String>,
    /// 本机 OCR 文字;None 表示不改动已有值
    pub ocr_text: Option<String>,
    /// 视觉模型描述;None 表示不改动已有值
    pub vision_desc: Option<String>,
    pub model: Option<String>,
    pub prompt_version: i64,
}

/// 统计信息
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub items: i64,
    pub notes: i64,
    pub images: i64,
    pub documents: i64,
    pub chunks: i64,
    /// 由非当前嵌入模型生成的切片数(需要重刷)
    pub stale_chunks: i64,
    pub pending_items: i64,
    pub failed_items: i64,
}

/// 标签统计
#[derive(Debug, Clone, Serialize)]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

/// 对话会话
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: i64,
    pub title: String,
    pub updated_at: i64,
    /// 该会话最近一次使用的对话模型引用
    pub model_ref: Option<String>,
    /// 该会话限定的知识库范围(空 = 搜索全部)
    pub kb_ids: Vec<i64>,
}

/// 知识库视图
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KbView {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub created_at: i64,
    /// 库内记录数
    pub item_count: i64,
}

/// 对话消息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub sources: Option<serde_json::Value>,
    pub created_at: i64,
    /// 回答所用模型名(仅助手消息)
    pub model: Option<String>,
    /// 回答所用来源:ollama / openai_compat
    pub provider: Option<String>,
    /// 回答总耗时(毫秒)
    pub elapsed_ms: Option<i64>,
    /// 首字耗时(毫秒)
    pub first_token_ms: Option<i64>,
}

/// 一次回答的模型与耗时信息
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnswerMeta {
    /// 模型名,如 qwen2.5:7b、deepseek-chat
    pub model: String,
    /// 来源:ollama / openai_compat
    pub provider: String,
    /// 云端档案的预设标识(用于挑选图标),本地为空
    #[serde(default)]
    pub preset: Option<String>,
    /// 总耗时(毫秒)
    pub elapsed_ms: i64,
    /// 首字耗时(毫秒);未产生文字时为空
    pub first_token_ms: Option<i64>,
}

/// 记录查询的公共 SELECT 片段
fn item_select(full_content: bool) -> String {
    let content_expr = if full_content {
        "i.content"
    } else {
        "substr(i.content, 1, 600)"
    };
    format!(
        "SELECT i.id, i.kind, i.title, {content_expr}, length(i.content), i.attachment_path, i.original_name, i.mime, \
         i.status, i.error, i.created_at, i.updated_at, m.summary, m.category, m.tags, m.entities, m.vision_text, \
         m.model, m.prompt_version, (SELECT count(*) FROM chunks c WHERE c.item_id = i.id), i.kb_id, i.source_path, m.ocr_text, i.embedded \
         FROM items i LEFT JOIN item_meta m ON m.item_id = i.id"
    )
}

/// 行映射为 ItemView
fn map_item(r: &Row) -> rusqlite::Result<ItemView> {
    let parse_list = |s: Option<String>| -> Vec<String> {
        s.and_then(|x| serde_json::from_str::<Vec<String>>(&x).ok())
            .unwrap_or_default()
    };
    Ok(ItemView {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        content: r.get(3)?,
        content_length: r.get(4)?,
        attachment_path: r.get(5)?,
        original_name: r.get(6)?,
        mime: r.get(7)?,
        status: r.get(8)?,
        error: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        summary: r.get(12)?,
        category: r.get(13)?,
        tags: parse_list(r.get(14)?),
        entities: parse_list(r.get(15)?),
        vision_text: r.get(16)?,
        meta_model: r.get(17)?,
        prompt_version: r.get(18)?,
        chunk_count: r.get(19)?,
        kb_id: r.get(20)?,
        source_path: r.get(21)?,
        ocr_text: r.get(22)?,
        embedded: r.get::<_, i64>(23)? != 0,
    })
}

/// 插入记录
///
/// 参数: c 连接/事务;it 新记录
/// 返回: 新记录 id
pub fn insert_item(c: &Connection, it: &NewItem) -> Result<i64> {
    let now = now_ms();
    c.execute(
        "INSERT INTO items(kind,title,content,attachment_path,original_name,mime,content_hash,status,created_at,updated_at,kb_id,source_path,embedded) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',?8,?8,?9,?10,?11)",
        params![
            it.kind,
            it.title,
            it.content,
            it.attachment_path,
            it.original_name,
            it.mime,
            it.content_hash,
            now,
            it.kb_id.unwrap_or(DEFAULT_KB_ID),
            it.source_path,
            it.embedded as i64
        ],
    )?;
    Ok(c.last_insert_rowid())
}

/// 读取单条记录(含完整原文)
///
/// 参数: c 连接;id 记录 id
/// 返回: 记录,不存在返回 None
pub fn get_item(c: &Connection, id: i64) -> Result<Option<ItemView>> {
    let sql = format!("{} WHERE i.id = ?1", item_select(true));
    Ok(c.query_row(&sql, [id], map_item).optional()?)
}

/// 列表查询(原文为预览)
///
/// 参数: c 连接;f 过滤条件
/// 返回: 按 id 倒序的记录
pub fn list_items(c: &Connection, f: &ItemFilter) -> Result<Vec<ItemView>> {
    let mut sql = format!("{} WHERE 1=1", item_select(false));
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(k) = f.kind.as_ref().filter(|s| !s.is_empty()) {
        // 支持逗号分隔的多种类型,如 "note,image"
        let kinds: Vec<&str> = k
            .split(',')
            .map(str::trim)
            .filter(|s| matches!(*s, "note" | "image" | "document"))
            .collect();
        if !kinds.is_empty() {
            sql.push_str(&format!(
                " AND i.kind IN ({})",
                vec!["?"; kinds.len()].join(",")
            ));
            for kd in kinds {
                args.push(kd.to_string().into());
            }
        }
    }
    if let Some(s) = f.status.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND i.status = ?");
        args.push(s.clone().into());
    }
    if let Some(t) = f.tag.as_ref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND instr(COALESCE(m.tags,''), ?) > 0");
        args.push(serde_json::to_string(t).unwrap_or_default().into());
    }
    if let Some(q) = f
        .search
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        sql.push_str(
            " AND instr(lower(COALESCE(i.content,'') || ' ' || COALESCE(i.title,'') || ' ' || COALESCE(i.original_name,'') || ' ' || \
             COALESCE(m.summary,'') || ' ' || COALESCE(m.tags,'') || ' ' || COALESCE(m.vision_text,'') || ' ' || COALESCE(i.source_path,'')), ?) > 0",
        );
        args.push(q.to_lowercase().into());
    }
    if let Some(kb) = f.kb_id {
        sql.push_str(" AND i.kb_id = ?");
        args.push(kb.into());
    }
    if let Some(b) = f.before_id {
        sql.push_str(" AND i.id < ?");
        args.push(b.into());
    }
    sql.push_str(" ORDER BY i.id DESC LIMIT ?");
    args.push(f.limit.unwrap_or(50).clamp(1, 200).into());
    let mut stmt = c.prepare_cached(&sql)?;
    let rows = stmt.query_map(params_from_iter(args), map_item)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 按内容哈希查找已有记录(用于附件去重)
///
/// 参数: c 连接;hash SHA-256;kind 记录类型;kb_id 知识库(去重只在库内进行)
/// 返回: 已存在的记录 id
pub fn find_by_hash(c: &Connection, hash: &str, kind: &str, kb_id: i64) -> Result<Option<i64>> {
    Ok(c.query_row(
        "SELECT id FROM items WHERE content_hash = ?1 AND kind = ?2 AND kb_id = ?3 LIMIT 1",
        params![hash, kind, kb_id],
        |r| r.get(0),
    )
    .optional()?)
}

/// 查找近期完全相同的笔记(防重复提交)
///
/// 参数: c 连接;content 原文;within_ms 时间窗口
/// 返回: 已存在的记录 id
pub fn find_recent_duplicate_note(
    c: &Connection,
    content: &str,
    within_ms: i64,
    kb_id: i64,
) -> Result<Option<i64>> {
    Ok(c.query_row(
        "SELECT id FROM items WHERE kind='note' AND content = ?1 AND created_at > ?2 AND kb_id = ?3 LIMIT 1",
        params![content, now_ms() - within_ms, kb_id],
        |r| r.get(0),
    )
    .optional()?)
}

/// 更新记录状态
pub fn set_item_status(c: &Connection, id: i64, status: &str, error: Option<&str>) -> Result<()> {
    c.execute(
        "UPDATE items SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, status, error, now_ms()],
    )?;
    Ok(())
}

/// 更新记录标题与原文(文档抽取完成后回写)
pub fn update_item_text(c: &Connection, id: i64, title: Option<&str>, content: &str) -> Result<()> {
    c.execute(
        "UPDATE items SET title = COALESCE(?2, title), content = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, title, content, now_ms()],
    )?;
    Ok(())
}

/// 删除记录及其 FTS 数据(chunks 与 item_meta 由外键级联)
///
/// 参数: c 连接/事务;id 记录 id
pub fn delete_item(c: &Connection, id: i64) -> Result<()> {
    c.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE item_id = ?1)",
        [id],
    )?;
    c.execute("DELETE FROM items WHERE id = ?1", [id])?;
    Ok(())
}

/// 写入/覆盖派生元数据
pub fn upsert_meta(c: &Connection, item_id: i64, m: &NewMeta) -> Result<()> {
    c.execute(
        "INSERT INTO item_meta(item_id,summary,category,tags,entities,vision_text,model,prompt_version,updated_at,ocr_text,vision_desc) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) \
         ON CONFLICT(item_id) DO UPDATE SET summary=excluded.summary, category=excluded.category, tags=excluded.tags, \
         entities=excluded.entities, vision_text=excluded.vision_text, model=excluded.model, \
         prompt_version=excluded.prompt_version, updated_at=excluded.updated_at, \
         ocr_text=COALESCE(excluded.ocr_text, item_meta.ocr_text), \
         vision_desc=COALESCE(excluded.vision_desc, item_meta.vision_desc)",
        params![
            item_id,
            m.summary,
            m.category,
            serde_json::to_string(&m.tags)?,
            serde_json::to_string(&m.entities)?,
            m.vision_text,
            m.model,
            m.prompt_version,
            now_ms(),
            m.ocr_text,
            m.vision_desc
        ],
    )?;
    Ok(())
}

/// 仅更新视觉解析文本(图片视觉阶段完成后立即持久化)
pub fn set_vision_text(c: &Connection, item_id: i64, text: &str) -> Result<()> {
    c.execute(
        "INSERT INTO item_meta(item_id, vision_text, updated_at) VALUES(?1,?2,?3) \
         ON CONFLICT(item_id) DO UPDATE SET vision_text=excluded.vision_text, updated_at=excluded.updated_at",
        params![item_id, text, now_ms()],
    )?;
    Ok(())
}

/// 用新切片整体替换某记录的旧切片(调用方应放在事务中)
///
/// 参数: c 连接/事务;item_id 记录 id;chunks 新切片
/// 返回: 新切片 id 列表(与输入顺序一致)
pub fn replace_chunks(c: &Connection, item_id: i64, chunks: &[NewChunk]) -> Result<Vec<i64>> {
    c.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE item_id = ?1)",
        [item_id],
    )?;
    c.execute("DELETE FROM chunks WHERE item_id = ?1", [item_id])?;
    let mut ids = Vec::with_capacity(chunks.len());
    for (seq, ch) in chunks.iter().enumerate() {
        c.execute(
            "INSERT INTO chunks(item_id,seq,text,heading,doc_summary,embed_model,dim,embedding,page) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![item_id, seq as i64, ch.text, ch.heading, ch.doc_summary, ch.embed_model, ch.dim as i64, ch.embedding, ch.page],
        )?;
        let id = c.last_insert_rowid();
        c.execute(
            "INSERT INTO chunks_fts(rowid, tokens) VALUES(?1, ?2)",
            params![id, ch.fts_tokens],
        )?;
        ids.push(id);
    }
    Ok(ids)
}

/// 清空某记录的全部切片(例如图片无文字可索引)
pub fn clear_chunks(c: &Connection, item_id: i64) -> Result<()> {
    replace_chunks(c, item_id, &[]).map(|_| ())
}

/// 读取某记录的切片
pub fn list_chunks(c: &Connection, item_id: i64) -> Result<Vec<ChunkView>> {
    let mut stmt = c.prepare_cached("SELECT id, seq, text, heading, doc_summary, embed_model, page FROM chunks WHERE item_id = ?1 ORDER BY seq")?;
    let rows = stmt.query_map([item_id], |r| {
        Ok(ChunkView {
            id: r.get(0)?,
            seq: r.get(1)?,
            text: r.get(2)?,
            heading: r.get(3)?,
            doc_summary: r.get(4)?,
            embed_model: r.get(5)?,
            page: r.get(6)?,
            images: vec![],
        })
    })?;
    let mut chunks = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    let ids: Vec<i64> = chunks.iter().map(|c| c.id).collect();
    let mut imgs = chunk_image_refs(c, &ids)?;
    for ch in &mut chunks {
        ch.images = imgs.remove(&ch.id).unwrap_or_default();
    }
    Ok(chunks)
}

/// 批量读取切片里出现的图片(含附件路径,用于展示缩略图)
pub fn chunk_image_refs(
    c: &Connection,
    chunk_ids: &[i64],
) -> Result<std::collections::HashMap<i64, Vec<ImageRef>>> {
    let mut map: std::collections::HashMap<i64, Vec<ImageRef>> = std::collections::HashMap::new();
    if chunk_ids.is_empty() {
        return Ok(map);
    }
    let sql = format!(
        "SELECT ci.chunk_id, i.id, i.attachment_path, COALESCE(i.original_name, i.title) \
         FROM chunk_images ci JOIN items i ON i.id = ci.image_item_id \
         WHERE ci.chunk_id IN ({}) ORDER BY ci.rowid",
        vec!["?"; chunk_ids.len()].join(",")
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(chunk_ids.iter()), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            ImageRef {
                id: r.get(1)?,
                attachment_path: r.get(2)?,
                name: r.get(3)?,
            },
        ))
    })?;
    for row in rows {
        let (ch, img) = row?;
        map.entry(ch).or_default().push(img);
    }
    Ok(map)
}

/// 统计信息
///
/// 参数: c 连接;embed_model 当前嵌入模型
pub fn stats(c: &Connection, embed_model: &str) -> Result<Stats> {
    stats_in(c, embed_model, None)
}

/// 统计信息(可限定知识库)
///
/// 参数: c 连接;embed_model 当前嵌入模型;kb_id 限定知识库(None = 全部)
pub fn stats_in(c: &Connection, embed_model: &str, kb_id: Option<i64>) -> Result<Stats> {
    // kb_id 为 -1 表示不过滤:所有查询统一带上 (?1 = -1 OR i.kb_id = ?1)
    let kb = kb_id.unwrap_or(-1);
    let count = |extra: &str| -> rusqlite::Result<i64> {
        c.query_row(
            &format!("SELECT count(*) FROM items i WHERE (?1 = -1 OR i.kb_id = ?1){extra}"),
            [kb],
            |r| r.get(0),
        )
    };
    Ok(Stats {
        items: count("")?,
        notes: count(" AND i.kind='note'")?,
        images: count(" AND i.kind='image'")?,
        documents: count(" AND i.kind='document'")?,
        chunks: c.query_row(
            "SELECT count(*) FROM chunks ch JOIN items i ON i.id = ch.item_id WHERE (?1 = -1 OR i.kb_id = ?1)",
            [kb],
            |r| r.get(0),
        )?,
        stale_chunks: c.query_row(
            "SELECT count(*) FROM chunks ch JOIN items i ON i.id = ch.item_id \
             WHERE (ch.embed_model IS NULL OR ch.embed_model != ?2) AND (?1 = -1 OR i.kb_id = ?1)",
            params![kb, embed_model],
            |r| r.get(0),
        )?,
        pending_items: count(" AND i.status IN ('pending','processing')")?,
        failed_items: count(" AND i.status = 'failed'")?,
    })
}

/// 统计标签出现次数(按次数降序,最多 60 个)
pub fn tag_counts(c: &Connection) -> Result<Vec<TagCount>> {
    let mut stmt = c.prepare("SELECT tags FROM item_meta WHERE tags != '[]'")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut map: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for row in rows {
        if let Ok(tags) = serde_json::from_str::<Vec<String>>(&row?) {
            for t in tags {
                *map.entry(t).or_insert(0) += 1;
            }
        }
    }
    let mut v: Vec<TagCount> = map
        .into_iter()
        .map(|(tag, count)| TagCount { tag, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.tag.cmp(&b.tag)));
    v.truncate(60);
    Ok(v)
}

/// 读取设置 JSON
pub fn get_setting(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(
        c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?,
    )
}

/// 写入设置 JSON
pub fn set_setting(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// 创建对话会话
pub fn create_session(c: &Connection, title: &str) -> Result<i64> {
    let now = now_ms();
    c.execute(
        "INSERT INTO chat_sessions(title,created_at,updated_at) VALUES(?1,?2,?2)",
        params![title, now],
    )?;
    Ok(c.last_insert_rowid())
}

/// 列出会话(最近更新在前)
pub fn list_sessions(c: &Connection) -> Result<Vec<SessionView>> {
    let mut stmt = c.prepare(
        "SELECT id, title, updated_at, model_ref, kb_ids FROM chat_sessions ORDER BY updated_at DESC LIMIT 100",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SessionView {
            id: r.get(0)?,
            title: r.get(1)?,
            updated_at: r.get(2)?,
            model_ref: r.get(3)?,
            kb_ids: parse_kb_ids(r.get::<_, Option<String>>(4)?.as_deref()),
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 读取会话消息
pub fn list_messages(c: &Connection, session_id: i64) -> Result<Vec<MessageView>> {
    let mut stmt = c.prepare_cached("SELECT id, role, content, sources, created_at, model, provider, elapsed_ms, first_token_ms FROM chat_messages WHERE session_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([session_id], |r| {
        let sources: Option<String> = r.get(3)?;
        Ok(MessageView {
            id: r.get(0)?,
            role: r.get(1)?,
            content: r.get(2)?,
            sources: sources.and_then(|s| serde_json::from_str(&s).ok()),
            created_at: r.get(4)?,
            model: r.get(5)?,
            provider: r.get(6)?,
            elapsed_ms: r.get(7)?,
            first_token_ms: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 追加一条助手回答,同时记录所用模型与耗时
///
/// 参数: c 连接;session_id 会话;content 回答;sources 资料;meta 模型与耗时
/// 返回: 消息 id
pub fn add_answer(
    c: &Connection,
    session_id: i64,
    content: &str,
    sources: Option<&serde_json::Value>,
    meta: &AnswerMeta,
) -> Result<i64> {
    let id = add_message(c, session_id, "assistant", content, sources)?;
    c.execute(
        "UPDATE chat_messages SET model=?2, provider=?3, elapsed_ms=?4, first_token_ms=?5 WHERE id=?1",
        params![id, meta.model, meta.provider, meta.elapsed_ms, meta.first_token_ms],
    )?;
    Ok(id)
}

/// 记住会话最近使用的模型
pub fn set_session_model(c: &Connection, session_id: i64, model_ref: &str) -> Result<()> {
    c.execute(
        "UPDATE chat_sessions SET model_ref = ?2 WHERE id = ?1",
        params![session_id, model_ref],
    )?;
    Ok(())
}

/// 解析会话保存的知识库范围 JSON(损坏或为空时视为"全部")
fn parse_kb_ids(s: Option<&str>) -> Vec<i64> {
    s.and_then(|x| serde_json::from_str::<Vec<i64>>(x).ok())
        .unwrap_or_default()
}

/// 记住会话限定的知识库范围(空 = 全部)
pub fn set_session_kbs(c: &Connection, session_id: i64, kb_ids: &[i64]) -> Result<()> {
    let json = if kb_ids.is_empty() {
        None
    } else {
        Some(serde_json::to_string(kb_ids)?)
    };
    c.execute(
        "UPDATE chat_sessions SET kb_ids = ?2 WHERE id = ?1",
        params![session_id, json],
    )?;
    Ok(())
}

/// 读取会话限定的知识库范围
pub fn get_session_kbs(c: &Connection, session_id: i64) -> Result<Vec<i64>> {
    let s: Option<Option<String>> = c
        .query_row(
            "SELECT kb_ids FROM chat_sessions WHERE id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(parse_kb_ids(s.flatten().as_deref()))
}

/// 追加消息并刷新会话时间
pub fn add_message(
    c: &Connection,
    session_id: i64,
    role: &str,
    content: &str,
    sources: Option<&serde_json::Value>,
) -> Result<i64> {
    let now = now_ms();
    c.execute(
        "INSERT INTO chat_messages(session_id,role,content,sources,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![session_id, role, content, sources.map(|s| s.to_string()), now],
    )?;
    let id = c.last_insert_rowid();
    c.execute(
        "UPDATE chat_sessions SET updated_at = ?2 WHERE id = ?1",
        params![session_id, now],
    )?;
    Ok(id)
}

/// 删除会话
pub fn delete_session(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM chat_sessions WHERE id = ?1", [id])?;
    Ok(())
}

/// 检查记录是否存在,不存在返回 Invalid
pub fn ensure_item_exists(c: &Connection, id: i64) -> Result<()> {
    let n: i64 = c.query_row("SELECT count(*) FROM items WHERE id = ?1", [id], |r| {
        r.get(0)
    })?;
    if n == 0 {
        return Err(AppError::Invalid(format!("记录 {id} 不存在")));
    }
    Ok(())
}

/// 校验并规整知识库名称
fn clean_kb_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::Invalid("知识库名称不能为空".into()));
    }
    if n.chars().count() > 40 {
        return Err(AppError::Invalid("知识库名称不能超过 40 个字".into()));
    }
    Ok(n.to_string())
}

/// 把 SQLite 唯一约束错误转为友好提示
fn kb_name_conflict(e: rusqlite::Error, name: &str) -> AppError {
    match &e {
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            AppError::Invalid(format!("已存在名为「{name}」的知识库"))
        }
        _ => e.into(),
    }
}

/// 列出全部知识库(默认库在前,其余按创建顺序)及各库记录数
pub fn list_kbs(c: &Connection) -> Result<Vec<KbView>> {
    let mut stmt = c.prepare(
        "SELECT k.id, k.name, k.description, k.created_at, \
         (SELECT count(*) FROM items i WHERE i.kb_id = k.id) \
         FROM knowledge_bases k ORDER BY k.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(KbView {
            id: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
            created_at: r.get(3)?,
            item_count: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 知识库是否存在
pub fn kb_exists(c: &Connection, id: i64) -> Result<bool> {
    let n: i64 = c.query_row(
        "SELECT count(*) FROM knowledge_bases WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 校验知识库存在,不存在返回 Invalid
pub fn ensure_kb_exists(c: &Connection, id: i64) -> Result<()> {
    if !kb_exists(c, id)? {
        return Err(AppError::Invalid(format!("知识库 {id} 不存在")));
    }
    Ok(())
}

/// 创建知识库
///
/// 参数: c 连接;name 名称(唯一,≤40 字);description 说明
/// 返回: 新知识库 id
pub fn create_kb(c: &Connection, name: &str, description: &str) -> Result<i64> {
    let name = clean_kb_name(name)?;
    c.execute(
        "INSERT INTO knowledge_bases(name, description, created_at) VALUES(?1, ?2, ?3)",
        params![name, description.trim(), now_ms()],
    )
    .map_err(|e| kb_name_conflict(e, &name))?;
    Ok(c.last_insert_rowid())
}

/// 重命名知识库(可同时改说明)
pub fn rename_kb(c: &Connection, id: i64, name: &str, description: Option<&str>) -> Result<()> {
    ensure_kb_exists(c, id)?;
    let name = clean_kb_name(name)?;
    c.execute(
        "UPDATE knowledge_bases SET name = ?2, description = COALESCE(?3, description) WHERE id = ?1",
        params![id, name, description.map(str::trim)],
    )
    .map_err(|e| kb_name_conflict(e, &name))?;
    Ok(())
}

/// 删除知识库行本身,并把会话范围里对它的引用移除
///
/// 调用方须先处理库内记录(转移到别的库或逐条删除);默认知识库不可删除。
pub fn drop_kb_row(c: &Connection, id: i64) -> Result<()> {
    if id == DEFAULT_KB_ID {
        return Err(AppError::Invalid("默认知识库不能删除".into()));
    }
    ensure_kb_exists(c, id)?;
    let left: i64 = c.query_row("SELECT count(*) FROM items WHERE kb_id = ?1", [id], |r| {
        r.get(0)
    })?;
    if left > 0 {
        return Err(AppError::Invalid("知识库内仍有记录,请先转移或删除".into()));
    }
    // 会话里引用了该库的范围,去掉这个 id
    let sessions: Vec<(i64, Option<String>)> = {
        let mut stmt =
            c.prepare("SELECT id, kb_ids FROM chat_sessions WHERE kb_ids IS NOT NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (sid, raw) in sessions {
        let ids = parse_kb_ids(raw.as_deref());
        if ids.contains(&id) {
            let kept: Vec<i64> = ids.into_iter().filter(|x| *x != id).collect();
            set_session_kbs(c, sid, &kept)?;
        }
    }
    c.execute("DELETE FROM knowledge_bases WHERE id = ?1", [id])?;
    Ok(())
}

/// 列出某知识库内全部记录 id
pub fn item_ids_in_kb(c: &Connection, kb_id: i64) -> Result<Vec<i64>> {
    let mut stmt = c.prepare("SELECT id FROM items WHERE kb_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([kb_id], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 把记录转移到另一知识库(调用方应放在同一个事务中)
///
/// 同库内已有相同内容哈希的记录时,该条会被跳过以避免库内出现重复文件。
///
/// 参数: c 连接/事务;ids 记录 id;kb_id 目标知识库
/// 返回: (实际转移的 id 列表, 因目标库已有同内容而跳过的 id 列表)
pub fn move_items(c: &Connection, ids: &[i64], kb_id: i64) -> Result<(Vec<i64>, Vec<i64>)> {
    ensure_kb_exists(c, kb_id)?;
    let mut moved = Vec::new();
    let mut skipped = Vec::new();
    for &id in ids {
        let Some((hash, kind, cur)): Option<(Option<String>, String, i64)> = c
            .query_row(
                "SELECT content_hash, kind, kb_id FROM items WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
        else {
            continue;
        };
        if cur == kb_id {
            continue;
        }
        if let Some(h) = hash.as_deref() {
            if find_by_hash(c, h, &kind, kb_id)?.is_some() {
                skipped.push(id);
                continue;
            }
        }
        c.execute(
            "UPDATE items SET kb_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, kb_id, now_ms()],
        )?;
        moved.push(id);
    }
    Ok((moved, skipped))
}

/// 查询一批主记录关联的子记录(如文档里引用的图片)
///
/// 参数: c 连接;parent_ids 主记录 id
/// 返回: 去重后的子记录 id
pub fn linked_children(c: &Connection, parent_ids: &[i64]) -> Result<Vec<i64>> {
    if parent_ids.is_empty() {
        return Ok(vec![]);
    }
    let sql = format!(
        "SELECT DISTINCT child_id FROM item_links WHERE relation='image' AND parent_id IN ({})",
        vec!["?"; parent_ids.len()].join(",")
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(parent_ids.iter()), |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 记录间的关联
///
/// 参数: parent 主记录;child 子记录;relation image / merged_from;anchor 原文中的引用写法;seq 序号
pub fn add_link(
    c: &Connection,
    parent: i64,
    child: i64,
    relation: &str,
    anchor: &str,
    seq: i64,
) -> Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO item_links(parent_id, child_id, relation, anchor, seq) VALUES(?1,?2,?3,?4,?5)",
        params![parent, child, relation, anchor, seq],
    )?;
    Ok(())
}

/// 文档引用的一张图片
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedImage {
    pub child_id: i64,
    /// 原文中的引用写法
    pub anchor: String,
    pub seq: i64,
    pub original_name: Option<String>,
    pub attachment_path: Option<String>,
    pub ocr_text: Option<String>,
    pub vision_desc: Option<String>,
}

/// 某文档引用的全部图片(按出现顺序)
pub fn images_of(c: &Connection, parent: i64) -> Result<Vec<LinkedImage>> {
    let mut stmt = c.prepare_cached(
        "SELECT l.child_id, l.anchor, l.seq, i.original_name, i.attachment_path, m.ocr_text, m.vision_desc \
         FROM item_links l JOIN items i ON i.id = l.child_id \
         LEFT JOIN item_meta m ON m.item_id = i.id \
         WHERE l.parent_id = ?1 AND l.relation = 'image' ORDER BY l.seq",
    )?;
    let rows = stmt.query_map([parent], |r| {
        Ok(LinkedImage {
            child_id: r.get(0)?,
            anchor: r.get(1)?,
            seq: r.get(2)?,
            original_name: r.get(3)?,
            attachment_path: r.get(4)?,
            ocr_text: r.get(5)?,
            vision_desc: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 引用某图片的文档
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentRef {
    pub id: i64,
    pub kind: String,
    pub title: Option<String>,
    pub original_name: Option<String>,
}

/// 某图片被哪些文档引用
pub fn parents_of(c: &Connection, child: i64) -> Result<Vec<ParentRef>> {
    let mut stmt = c.prepare_cached(
        "SELECT DISTINCT i.id, i.kind, i.title, i.original_name FROM item_links l \
         JOIN items i ON i.id = l.parent_id WHERE l.child_id = ?1 AND l.relation = 'image' ORDER BY i.id",
    )?;
    let rows = stmt.query_map([child], |r| {
        Ok(ParentRef {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            original_name: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 单独写入本机 OCR 文字(空串表示已识别但没有文字,避免反复识别)
pub fn set_ocr_text(c: &Connection, item_id: i64, text: &str) -> Result<()> {
    c.execute(
        "INSERT INTO item_meta(item_id, ocr_text, updated_at) VALUES(?1,?2,?3) \
         ON CONFLICT(item_id) DO UPDATE SET ocr_text=excluded.ocr_text",
        params![item_id, text, now_ms()],
    )?;
    Ok(())
}

/// 读取本机 OCR 文字;None 表示尚未识别
pub fn get_ocr_text(c: &Connection, item_id: i64) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT ocr_text FROM item_meta WHERE item_id = ?1",
        [item_id],
        |r| r.get::<_, Option<String>>(0),
    )
    .optional()?
    .flatten())
}

/// 用 (切片 id, 图片 id) 关系覆盖写入(旧切片被替换时关系已随外键级联删除)
pub fn add_chunk_images(c: &Connection, pairs: &[(i64, i64)]) -> Result<()> {
    for (chunk_id, image_id) in pairs {
        // 图片可能在处理期间被删除:不存在则忽略
        c.execute(
            "INSERT OR IGNORE INTO chunk_images(chunk_id, image_item_id) \
             SELECT ?1, id FROM items WHERE id = ?2",
            params![chunk_id, image_id],
        )?;
    }
    Ok(())
}

/// 用户显式导入了一张此前由文档内嵌自动导入的图片:转为普通图片(不再随文档清理)
pub fn promote_embedded(c: &Connection, id: i64) -> Result<()> {
    c.execute("UPDATE items SET embedded = 0 WHERE id = ?1", [id])?;
    Ok(())
}

/// 从候选图片里挑出"内嵌导入且已无任何文档引用"的孤儿图片
pub fn orphan_embedded(c: &Connection, candidates: &[i64]) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for &id in candidates {
        let n: i64 = c.query_row(
            "SELECT count(*) FROM items i WHERE i.id = ?1 AND i.embedded = 1 \
             AND NOT EXISTS (SELECT 1 FROM item_links l WHERE l.child_id = i.id AND l.relation = 'image')",
            [id],
            |r| r.get(0),
        )?;
        if n > 0 {
            out.push(id);
        }
    }
    Ok(out)
}

/// 统计引用某附件路径的记录数(删除记录时判断能否删除文件)
pub fn count_attachment_refs(c: &Connection, path: &str) -> Result<i64> {
    Ok(c.query_row(
        "SELECT count(*) FROM items WHERE attachment_path = ?1",
        [path],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn note(db: &Db, text: &str) -> i64 {
        db.with(|c| {
            insert_item(
                c,
                &NewItem {
                    kind: "note".into(),
                    content: text.into(),
                    ..Default::default()
                },
            )
        })
        .unwrap()
    }

    #[test]
    fn 插入与读取() {
        let db = Db::open_memory().unwrap();
        let id = note(&db, "你好");
        let it = db.with(|c| get_item(c, id)).unwrap().unwrap();
        assert_eq!(it.content, "你好");
        assert_eq!(it.status, "pending");
    }

    #[test]
    fn 过滤搜索与标签() {
        let db = Db::open_memory().unwrap();
        let a = note(&db, "A 项目预算");
        let b = note(&db, "买牛奶");
        db.with(|c| {
            upsert_meta(
                c,
                a,
                &NewMeta {
                    tags: vec!["项目A".into()],
                    prompt_version: 1,
                    ..Default::default()
                },
            )?;
            upsert_meta(
                c,
                b,
                &NewMeta {
                    tags: vec!["生活".into()],
                    prompt_version: 1,
                    ..Default::default()
                },
            )
        })
        .unwrap();
        let r = db
            .with(|c| {
                list_items(
                    c,
                    &ItemFilter {
                        tag: Some("项目A".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, a);
        let r = db
            .with(|c| {
                list_items(
                    c,
                    &ItemFilter {
                        search: Some("牛奶".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(r.len(), 1);
        // 搜索词包含通配符不应报错或误匹配
        let r = db
            .with(|c| {
                list_items(
                    c,
                    &ItemFilter {
                        search: Some("100%_".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn 替换切片会同步fts并可级联删除() {
        let db = Db::open_memory().unwrap();
        let id = note(&db, "x");
        let mk = |t: &str| NewChunk {
            text: t.into(),
            heading: String::new(),
            doc_summary: None,
            embed_model: "m".into(),
            embedding: vec![0, 0, 128, 63],
            dim: 1,
            page: None,
            fts_tokens: t.into(),
        };
        db.with_tx(|tx| replace_chunks(tx, id, &[mk("预算"), mk("缩减")]).map(|_| ()))
            .unwrap();
        db.with_tx(|tx| replace_chunks(tx, id, &[mk("新内容")]).map(|_| ()))
            .unwrap();
        let (chunks, fts): (i64, i64) = db
            .with(|c| {
                Ok((
                    c.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?,
                    c.query_row("SELECT count(*) FROM chunks_fts", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((chunks, fts), (1, 1));
        db.with_tx(|tx| delete_item(tx, id)).unwrap();
        let (chunks, fts): (i64, i64) = db
            .with(|c| {
                Ok((
                    c.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?,
                    c.query_row("SELECT count(*) FROM chunks_fts", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((chunks, fts), (0, 0));
    }

    #[test]
    fn 统计识别过期向量() {
        let db = Db::open_memory().unwrap();
        let id = note(&db, "x");
        db.with_tx(|tx| {
            replace_chunks(
                tx,
                id,
                &[NewChunk {
                    text: "a".into(),
                    heading: String::new(),
                    doc_summary: None,
                    embed_model: "old".into(),
                    embedding: vec![],
                    dim: 0,
                    page: None,
                    fts_tokens: "a".into(),
                }],
            )
            .map(|_| ())
        })
        .unwrap();
        let s = db.with(|c| stats(c, "new")).unwrap();
        assert_eq!(s.stale_chunks, 1);
        let s = db.with(|c| stats(c, "old")).unwrap();
        assert_eq!(s.stale_chunks, 0);
    }

    #[test]
    fn 会话与消息() {
        let db = Db::open_memory().unwrap();
        let sid = db.with(|c| create_session(c, "测试")).unwrap();
        db.with(|c| add_message(c, sid, "user", "hi", None))
            .unwrap();
        db.with(|c| {
            add_message(
                c,
                sid,
                "assistant",
                "yo",
                Some(&serde_json::json!([{"n":1}])),
            )
        })
        .unwrap();
        let msgs = db.with(|c| list_messages(c, sid)).unwrap();
        assert_eq!(msgs.len(), 2);
        assert!(msgs[1].sources.is_some());
        db.with(|c| delete_session(c, sid)).unwrap();
        assert!(db.with(|c| list_messages(c, sid)).unwrap().is_empty());
    }

    fn note_in(db: &Db, text: &str, kb: i64, hash: Option<&str>) -> i64 {
        db.with(|c| {
            insert_item(
                c,
                &NewItem {
                    kind: "document".into(),
                    content: text.into(),
                    content_hash: hash.map(Into::into),
                    kb_id: Some(kb),
                    ..Default::default()
                },
            )
        })
        .unwrap()
    }

    #[test]
    fn 知识库增删改与默认库保护() {
        let db = Db::open_memory().unwrap();
        let kbs = db.with(list_kbs).unwrap();
        assert_eq!(kbs.len(), 1);
        assert_eq!(kbs[0].id, DEFAULT_KB_ID);
        let a = db.with(|c| create_kb(c, " 工作 ", "项目资料")).unwrap();
        // 重名、空名、过长名均被拒绝
        assert!(db.with(|c| create_kb(c, "工作", "")).is_err());
        assert!(db.with(|c| create_kb(c, "  ", "")).is_err());
        assert!(db.with(|c| create_kb(c, &"长".repeat(41), "")).is_err());
        db.with(|c| rename_kb(c, a, "工作资料", None)).unwrap();
        let kbs = db.with(list_kbs).unwrap();
        assert_eq!(kbs[1].name, "工作资料");
        assert_eq!(kbs[1].description, "项目资料");
        // 默认库不可删除;有记录的库不能直接删
        assert!(db.with(|c| drop_kb_row(c, DEFAULT_KB_ID)).is_err());
        let it = note_in(&db, "x", a, None);
        assert!(db.with(|c| drop_kb_row(c, a)).is_err());
        db.with_tx(|tx| move_items(tx, &[it], DEFAULT_KB_ID).map(|_| ()))
            .unwrap();
        db.with(|c| drop_kb_row(c, a)).unwrap();
        assert_eq!(db.with(list_kbs).unwrap().len(), 1);
    }

    #[test]
    fn 列表与统计按知识库过滤() {
        let db = Db::open_memory().unwrap();
        let b = db.with(|c| create_kb(c, "B", "")).unwrap();
        note_in(&db, "a1", DEFAULT_KB_ID, None);
        note_in(&db, "b1", b, None);
        note_in(&db, "b2", b, None);
        let in_b = db
            .with(|c| {
                list_items(
                    c,
                    &ItemFilter {
                        kb_id: Some(b),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(in_b.len(), 2);
        assert!(in_b.iter().all(|i| i.kb_id == b));
        let all = db.with(|c| list_items(c, &ItemFilter::default())).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(db.with(|c| stats_in(c, "m", Some(b))).unwrap().items, 2);
        assert_eq!(db.with(|c| stats_in(c, "m", None)).unwrap().items, 3);
        let kbs = db.with(list_kbs).unwrap();
        assert_eq!(kbs.iter().find(|k| k.id == b).unwrap().item_count, 2);
    }

    #[test]
    fn 去重只在库内进行且转移时跳过同库重复() {
        let db = Db::open_memory().unwrap();
        let b = db.with(|c| create_kb(c, "B", "")).unwrap();
        let a_item = note_in(&db, "同一个文件", DEFAULT_KB_ID, Some("h1"));
        // 同哈希在另一个库不算重复
        assert_eq!(
            db.with(|c| find_by_hash(c, "h1", "document", b)).unwrap(),
            None
        );
        let b_item = note_in(&db, "同一个文件", b, Some("h1"));
        assert_eq!(
            db.with(|c| find_by_hash(c, "h1", "document", b)).unwrap(),
            Some(b_item)
        );
        // 把默认库那份转移进 B:B 已有同内容,被跳过且不改归属
        let (moved, skipped) = db.with_tx(|tx| move_items(tx, &[a_item], b)).unwrap();
        assert!(moved.is_empty());
        assert_eq!(skipped, vec![a_item]);
        let it = db.with(|c| get_item(c, a_item)).unwrap().unwrap();
        assert_eq!(it.kb_id, DEFAULT_KB_ID);
    }

    #[test]
    fn 转移到不存在的库会失败且不改动数据() {
        let db = Db::open_memory().unwrap();
        let it = note_in(&db, "x", DEFAULT_KB_ID, None);
        assert!(db
            .with_tx(|tx| move_items(tx, &[it], 999).map(|_| ()))
            .is_err());
        assert_eq!(
            db.with(|c| get_item(c, it)).unwrap().unwrap().kb_id,
            DEFAULT_KB_ID
        );
    }

    #[test]
    fn 会话范围保存读取且删库时移除引用() {
        let db = Db::open_memory().unwrap();
        let b = db.with(|c| create_kb(c, "B", "")).unwrap();
        let sid = db.with(|c| create_session(c, "s")).unwrap();
        assert!(db.with(|c| get_session_kbs(c, sid)).unwrap().is_empty());
        db.with(|c| set_session_kbs(c, sid, &[DEFAULT_KB_ID, b]))
            .unwrap();
        assert_eq!(
            db.with(|c| get_session_kbs(c, sid)).unwrap(),
            vec![DEFAULT_KB_ID, b]
        );
        assert_eq!(
            db.with(list_sessions).unwrap()[0].kb_ids,
            vec![DEFAULT_KB_ID, b]
        );
        db.with(|c| drop_kb_row(c, b)).unwrap();
        assert_eq!(
            db.with(|c| get_session_kbs(c, sid)).unwrap(),
            vec![DEFAULT_KB_ID]
        );
        // 清空范围 = 全部
        db.with(|c| set_session_kbs(c, sid, &[])).unwrap();
        assert!(db.with(|c| get_session_kbs(c, sid)).unwrap().is_empty());
    }
}
