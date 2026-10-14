// @author ygw
//! 数据访问层:记录、元数据、切片、设置、对话的 SQL 封装。

use crate::db::now_ms;
use crate::error::{AppError, Result};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

/// 记录视图(返回给前端,camelCase)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: i64,
    pub kind: String,
    pub title: Option<String>,
    /// 原文(列表场景为前 600 字的预览)
    pub content: String,
    /// 原文总字符数
    pub content_length: i64,
    pub attachment_path: Option<String>,
    pub original_name: Option<String>,
    pub mime: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub summary: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub entities: Vec<String>,
    pub vision_text: Option<String>,
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
         m.model, m.prompt_version, (SELECT count(*) FROM chunks c WHERE c.item_id = i.id) \
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
    })
}

/// 插入记录
///
/// 参数: c 连接/事务;it 新记录
/// 返回: 新记录 id
pub fn insert_item(c: &Connection, it: &NewItem) -> Result<i64> {
    let now = now_ms();
    c.execute(
        "INSERT INTO items(kind,title,content,attachment_path,original_name,mime,content_hash,status,created_at,updated_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',?8,?8)",
        params![it.kind, it.title, it.content, it.attachment_path, it.original_name, it.mime, it.content_hash, now],
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
             COALESCE(m.summary,'') || ' ' || COALESCE(m.tags,'') || ' ' || COALESCE(m.vision_text,'')), ?) > 0",
        );
        args.push(q.to_lowercase().into());
    }
    if let Some(b) = f.before_id {
        sql.push_str(" AND i.id < ?");
        args.push(b.into());
    }
    sql.push_str(" ORDER BY i.id DESC LIMIT ?");
    args.push(f.limit.unwrap_or(50).clamp(1, 200).into());
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args), map_item)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 按内容哈希查找已有记录(用于附件去重)
///
/// 参数: c 连接;hash SHA-256;kind 记录类型
/// 返回: 已存在的记录 id
pub fn find_by_hash(c: &Connection, hash: &str, kind: &str) -> Result<Option<i64>> {
    Ok(c.query_row(
        "SELECT id FROM items WHERE content_hash = ?1 AND kind = ?2 LIMIT 1",
        params![hash, kind],
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
) -> Result<Option<i64>> {
    Ok(c.query_row(
        "SELECT id FROM items WHERE kind='note' AND content = ?1 AND created_at > ?2 LIMIT 1",
        params![content, now_ms() - within_ms],
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
        "INSERT INTO item_meta(item_id,summary,category,tags,entities,vision_text,model,prompt_version,updated_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(item_id) DO UPDATE SET summary=excluded.summary, category=excluded.category, tags=excluded.tags, \
         entities=excluded.entities, vision_text=excluded.vision_text, model=excluded.model, \
         prompt_version=excluded.prompt_version, updated_at=excluded.updated_at",
        params![
            item_id,
            m.summary,
            m.category,
            serde_json::to_string(&m.tags)?,
            serde_json::to_string(&m.entities)?,
            m.vision_text,
            m.model,
            m.prompt_version,
            now_ms()
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
            "INSERT INTO chunks(item_id,seq,text,heading,doc_summary,embed_model,dim,embedding) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![item_id, seq as i64, ch.text, ch.heading, ch.doc_summary, ch.embed_model, ch.dim as i64, ch.embedding],
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
    let mut stmt = c.prepare("SELECT id, seq, text, heading, doc_summary, embed_model FROM chunks WHERE item_id = ?1 ORDER BY seq")?;
    let rows = stmt.query_map([item_id], |r| {
        Ok(ChunkView {
            id: r.get(0)?,
            seq: r.get(1)?,
            text: r.get(2)?,
            heading: r.get(3)?,
            doc_summary: r.get(4)?,
            embed_model: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 统计信息
///
/// 参数: c 连接;embed_model 当前嵌入模型
pub fn stats(c: &Connection, embed_model: &str) -> Result<Stats> {
    let count = |sql: &str| -> rusqlite::Result<i64> { c.query_row(sql, [], |r| r.get(0)) };
    Ok(Stats {
        items: count("SELECT count(*) FROM items")?,
        notes: count("SELECT count(*) FROM items WHERE kind='note'")?,
        images: count("SELECT count(*) FROM items WHERE kind='image'")?,
        documents: count("SELECT count(*) FROM items WHERE kind='document'")?,
        chunks: count("SELECT count(*) FROM chunks")?,
        stale_chunks: c.query_row(
            "SELECT count(*) FROM chunks WHERE embed_model IS NULL OR embed_model != ?1",
            [embed_model],
            |r| r.get(0),
        )?,
        pending_items: count(
            "SELECT count(*) FROM items WHERE status IN ('pending','processing')",
        )?,
        failed_items: count("SELECT count(*) FROM items WHERE status = 'failed'")?,
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
        "SELECT id, title, updated_at FROM chat_sessions ORDER BY updated_at DESC LIMIT 100",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SessionView {
            id: r.get(0)?,
            title: r.get(1)?,
            updated_at: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// 读取会话消息
pub fn list_messages(c: &Connection, session_id: i64) -> Result<Vec<MessageView>> {
    let mut stmt = c.prepare("SELECT id, role, content, sources, created_at FROM chat_messages WHERE session_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([session_id], |r| {
        let sources: Option<String> = r.get(3)?;
        Ok(MessageView {
            id: r.get(0)?,
            role: r.get(1)?,
            content: r.get(2)?,
            sources: sources.and_then(|s| serde_json::from_str(&s).ok()),
            created_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
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
}
