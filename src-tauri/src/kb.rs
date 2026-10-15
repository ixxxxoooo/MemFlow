// @author ygw
//! 知识库管理:新建/重命名/删除、记录在知识库之间转移。
//!
//! 与 RAGFlow 的 dataset 概念对应:每个知识库是一个独立的检索范围。
//! 转移记录时只改归属(数据库 + 向量块上的 kb_id),不重新嵌入,所以是瞬时操作。

use crate::core::Core;
use crate::db::DEFAULT_KB_ID;
use crate::error::Result;
use crate::ingest;
use crate::repo::{self, KbView};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// 删除知识库时对库内记录的处理方式
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeleteMode {
    /// 把记录转移到默认知识库后再删除
    MoveToDefault,
    /// 连同库内记录一起删除
    Cascade,
}

/// 转移结果
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MoveResult {
    /// 已转移的记录 id(含随主记录一起转移的关联图片)
    pub moved: Vec<i64>,
    /// 目标库已有相同内容而跳过的记录 id
    pub skipped: Vec<i64>,
}

/// 列出知识库
pub fn list(core: &Core) -> Result<Vec<KbView>> {
    core.db.with_read(repo::list_kbs)
}

/// 新建知识库
pub fn create(core: &Core, name: &str, description: &str) -> Result<KbView> {
    let id = core.db.with(|c| repo::create_kb(c, name, description))?;
    let kb = core
        .db
        .with(repo::list_kbs)?
        .into_iter()
        .find(|k| k.id == id)
        .expect("刚创建的知识库必然存在");
    core.emit("kb-changed", json!({ "id": id }));
    Ok(kb)
}

/// 重命名知识库
pub fn rename(core: &Core, id: i64, name: &str, description: Option<&str>) -> Result<()> {
    core.db
        .with(|c| repo::rename_kb(c, id, name, description))?;
    core.emit("kb-changed", json!({ "id": id }));
    Ok(())
}

/// 把记录转移到另一个知识库
///
/// 参数: core 核心;ids 记录 id;kb_id 目标知识库
/// 返回: 转移与跳过的记录
pub fn move_items(core: &Arc<Core>, ids: &[i64], kb_id: i64) -> Result<MoveResult> {
    let (moved, skipped) = core.db.with_tx(|tx| {
        let (mut moved, skipped) = repo::move_items(tx, ids, kb_id)?;
        // 关联的子图片跟随主记录一起转移(图片关联表在阶段 2 引入,此处按表是否存在兼容)
        let children = repo::linked_children(tx, &moved)?;
        if !children.is_empty() {
            let (extra, _) = repo::move_items(tx, &children, kb_id)?;
            moved.extend(extra);
        }
        Ok((moved, skipped))
    })?;
    // 向量块只改归属,不重新嵌入
    core.vindex.set_items_kb(&moved, kb_id);
    for id in &moved {
        core.emit("item-updated", json!({ "id": id }));
    }
    core.emit("kb-changed", json!({ "id": kb_id }));
    Ok(MoveResult { moved, skipped })
}

/// 删除知识库
///
/// 参数: core 核心;id 知识库 id(默认库不可删);mode 库内记录的处理方式
/// 返回: 受影响的记录数(转移或删除的条数)
pub async fn delete(core: &Arc<Core>, id: i64, mode: DeleteMode) -> Result<usize> {
    core.db.with(|c| repo::ensure_kb_exists(c, id))?;
    if id == DEFAULT_KB_ID {
        return Err(crate::error::AppError::Invalid("默认知识库不能删除".into()));
    }
    let ids = core.db.with(|c| repo::item_ids_in_kb(c, id))?;
    let affected = match mode {
        DeleteMode::MoveToDefault => {
            let r = move_items(core, &ids, DEFAULT_KB_ID)?;
            // 默认库已有同内容的记录无法转移:这部分作为重复内容直接删除
            if !r.skipped.is_empty() {
                ingest::delete_items(core, &r.skipped).await?;
            }
            r.moved.len() + r.skipped.len()
        }
        DeleteMode::Cascade => ingest::delete_items(core, &ids).await?,
    };
    core.db.with(|c| repo::drop_kb_row(c, id))?;
    core.emit("kb-changed", json!({ "id": id }));
    Ok(affected)
}
