// @author ygw
//! 混合检索:向量相似度 + FTS5 全文(jieba 分词)+ RRF 融合 + 时间衰减。
//!
//! 健壮性:
//! - 向量化失败(Ollama 不可用)时自动降级为纯全文检索
//! - 同时设置相关性闸门,低相关内容不返回,配合问答"无资料则拒答"防止幻觉

use crate::core::Core;
use crate::error::Result;
use crate::tokenizer;
use rusqlite::params_from_iter;
use serde::Serialize;
use std::collections::HashMap;

/// RRF 平滑常数
const RRF_K: f32 = 60.0;
/// 每路召回数量
const RECALL_N: usize = 30;
/// 同一条记录最多保留的切片数
const MAX_CHUNKS_PER_ITEM: usize = 2;

/// 检索命中
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub chunk_id: i64,
    pub item_id: i64,
    pub kind: String,
    /// 所属知识库
    pub kb_id: i64,
    pub title: Option<String>,
    pub text: String,
    pub heading: Option<String>,
    /// 所在页码(PDF 文档)
    pub page: Option<i64>,
    pub doc_summary: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub attachment_path: Option<String>,
    /// 融合得分
    pub score: f32,
    /// 向量相似度(无则为 None)
    pub vec_score: Option<f32>,
}

/// 检索参数
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// 最终返回条数
    pub top_n: usize,
    /// 向量相关性阈值
    pub threshold: f32,
    /// 限定的知识库范围(None 或空 = 搜索全部);向量与全文两路都会硬过滤
    pub kb_ids: Option<Vec<i64>>,
}

impl SearchOptions {
    /// 规整后的知识库范围:空列表视为不限制
    fn kb_scope(&self) -> Option<&[i64]> {
        self.kb_ids.as_deref().filter(|v| !v.is_empty())
    }
}

/// 时间衰减系数:越新的记录类内容略微加权(最多 +20%),文档不衰减
///
/// 参数: kind 记录类型;created_at 创建时间(毫秒);now 当前时间(毫秒)
/// 返回: 乘数
pub fn recency_boost(kind: &str, created_at: i64, now: i64) -> f32 {
    if kind == "document" {
        return 1.0;
    }
    let age_days = ((now - created_at).max(0) as f32) / 86_400_000.0;
    1.0 + 0.2 * (-age_days / 60.0).exp()
}

/// RRF 融合:输入多路有序 id 列表,输出 id → 得分
///
/// 参数: lists 每路按相关性降序的 id 列表
/// 返回: 融合得分
pub fn rrf_fuse(lists: &[Vec<i64>]) -> HashMap<i64, f32> {
    let mut m: HashMap<i64, f32> = HashMap::new();
    for l in lists {
        for (rank, id) in l.iter().enumerate() {
            *m.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
    }
    m
}

/// 全文检索:返回按 BM25 排序的切片 id
fn fts_search(core: &Core, terms: &[String], kb_scope: Option<&[i64]>) -> Vec<i64> {
    let Some(expr) = tokenizer::fts_match_expr(terms) else {
        return vec![];
    };
    let r = core.db.with_read(|c| {
        let (sql, mut args): (String, Vec<rusqlite::types::Value>) = match kb_scope {
            // 限定知识库:联表 items 做硬过滤
            Some(kbs) => (
                format!(
                    "SELECT f.rowid FROM chunks_fts f JOIN chunks ch ON ch.id = f.rowid JOIN items i ON i.id = ch.item_id \
                     WHERE chunks_fts MATCH ?1 AND i.kb_id IN ({}) ORDER BY bm25(chunks_fts) LIMIT ?",
                    vec!["?"; kbs.len()].join(",")
                ),
                kbs.iter().map(|k| (*k).into()).collect(),
            ),
            None => (
                "SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY bm25(chunks_fts) LIMIT ?".into(),
                Vec::new(),
            ),
        };
        args.insert(0, expr.clone().into());
        args.push((RECALL_N as i64).into());
        let mut stmt = c.prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), |r| r.get::<_, i64>(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    });
    match r {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("FTS 查询失败,已忽略: {e}");
            vec![]
        }
    }
}

/// 读取切片详情
fn load_hits(core: &Core, ids: &[i64]) -> Result<HashMap<i64, Hit>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT c.id, c.item_id, i.kind, COALESCE(i.title, i.original_name), c.text, c.heading, c.doc_summary, \
         m.summary, m.tags, i.created_at, i.attachment_path, c.page, i.kb_id \
         FROM chunks c JOIN items i ON i.id = c.item_id LEFT JOIN item_meta m ON m.item_id = i.id \
         WHERE c.id IN ({placeholders})"
    );
    core.db.with_read(|c| {
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(ids.iter()), |r| {
            let tags: Option<String> = r.get(8)?;
            let heading: Option<String> = r.get(5)?;
            Ok(Hit {
                chunk_id: r.get(0)?,
                item_id: r.get(1)?,
                kind: r.get(2)?,
                kb_id: r.get(12)?,
                title: r.get(3)?,
                text: r.get(4)?,
                heading: heading.filter(|h| !h.is_empty()),
                page: r.get(11)?,
                doc_summary: r.get(6)?,
                summary: r.get(7)?,
                tags: tags
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_default(),
                created_at: r.get(9)?,
                attachment_path: r.get(10)?,
                score: 0.0,
                vec_score: None,
            })
        })?;
        let mut map = HashMap::new();
        for h in rows {
            let h = h?;
            map.insert(h.chunk_id, h);
        }
        Ok(map)
    })
}

/// 全文命中是否可信:切片文本(含标签/摘要/标题)至少包含半数以上的查询关键词
fn fts_hit_plausible(hit: &Hit, terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let hay = format!(
        "{} {} {} {}",
        hit.text,
        hit.title.clone().unwrap_or_default(),
        hit.summary.clone().unwrap_or_default(),
        hit.tags.join(" ")
    )
    .to_lowercase();
    let matched = terms.iter().filter(|t| hay.contains(t.as_str())).count();
    matched * 2 >= terms.len()
}

/// 重排序后的最终得分:重排序概率为主,时效加权只作为很弱的平局项(最多 +0.03)
///
/// 参数: rerank_prob 重排序概率(0~1);recency 时效乘数(1.0~1.2)
pub fn final_score(rerank_prob: f32, recency: f32) -> f32 {
    rerank_prob + 0.03 * ((recency - 1.0) / 0.2).clamp(0.0, 1.0)
}

/// 本次检索是否会执行重排序(已开启、模型已安装且候选不少于 2 条)
fn rerank_applicable(core: &Core, n: usize) -> bool {
    let s = core.settings();
    s.rerank_enabled && n >= 2 && crate::rerank::is_installed(&core.paths.root, &s.rerank_model)
}

/// 查询向量化(按设置走本地或云端)
async fn embed_query(core: &Core, query: &str) -> Result<Vec<Vec<f32>>> {
    core.embedder()?.embed(&[query.to_string()]).await
}

/// 用本地模型对候选重排序;未启用、未下载、失败或超时都保持原有 RRF 顺序
async fn rerank_hits(core: &Core, query: &str, hits: &mut [Hit]) {
    let s = core.settings();
    if !rerank_applicable(core, hits.len()) {
        return;
    }
    let docs: Vec<String> = hits
        .iter()
        .map(|h| match &h.heading {
            Some(hd) => format!("{} {}\n{}", h.title.clone().unwrap_or_default(), hd, h.text),
            None => format!("{}\n{}", h.title.clone().unwrap_or_default(), h.text),
        })
        .collect();
    let started = std::time::Instant::now();
    match core
        .reranker
        .score(
            &core.paths.root,
            &s.rerank_model,
            query,
            docs,
            crate::rerank::RERANK_BUDGET,
        )
        .await
    {
        Ok(scores) if scores.len() == hits.len() => {
            let now = crate::db::now_ms();
            for (h, p) in hits.iter_mut().zip(scores) {
                h.score = final_score(p, recency_boost(&h.kind, h.created_at, now));
            }
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            tracing::debug!("重排序 {} 条,耗时 {:?}", hits.len(), started.elapsed());
        }
        Ok(_) => tracing::warn!("重排序返回数量不一致,已忽略"),
        Err(e) => tracing::warn!("重排序失败,沿用 RRF 排序: {e}"),
    }
}

/// 混合检索
///
/// 参数: core 核心;query 查询文本;opts 检索参数
/// 返回: 按融合得分降序的命中(已应用相关性闸门与每记录切片数上限)
pub async fn hybrid_search(core: &Core, query: &str, opts: &SearchOptions) -> Result<Vec<Hit>> {
    hybrid_search_with_stage(core, query, opts, &|_| {}).await
}

/// 同 `hybrid_search`,并在各阶段开始时回调(retrieve 检索 / rerank 重排)
pub async fn hybrid_search_with_stage(
    core: &Core,
    query: &str,
    opts: &SearchOptions,
    on_stage: &(dyn Fn(&str) + Send + Sync),
) -> Result<Vec<Hit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(vec![]);
    }
    let settings = core.settings();

    // 向量路(失败降级);相同问题命中 LRU 缓存则免去向量化往返
    on_stage("retrieve");
    let mut vec_hits: Vec<crate::vector::VecHit> = Vec::new();
    let identity = settings.embed_identity();
    let cached = core.query_vecs.lock().unwrap().get(&identity, query);
    let qvec = match cached {
        Some(v) => Some(v),
        None => match embed_query(core, query).await {
            Ok(mut v) if !v.is_empty() => {
                let v = v.remove(0);
                core.query_vecs
                    .lock()
                    .unwrap()
                    .put(&identity, query, v.clone());
                Some(v)
            }
            Ok(_) => None,
            Err(e) => {
                tracing::warn!("查询向量化失败,降级为全文检索: {e}");
                None
            }
        },
    };
    if let Some(v) = qvec {
        vec_hits = core.vindex.search_in(&v, RECALL_N, opts.kb_scope());
    }
    let vec_scores: HashMap<i64, f32> = vec_hits.iter().map(|h| (h.chunk_id, h.score)).collect();

    // 全文路
    let terms = tokenizer::query_terms(query);
    let fts_ids = fts_search(core, &terms, opts.kb_scope());

    // 融合
    let vec_ids: Vec<i64> = vec_hits.iter().map(|h| h.chunk_id).collect();
    let fused = rrf_fuse(&[vec_ids.clone(), fts_ids.clone()]);
    let mut all_ids: Vec<i64> = fused.keys().copied().collect();
    all_ids.sort_unstable();
    let mut details = load_hits(core, &all_ids)?;
    let now = crate::db::now_ms();
    let fts_set: std::collections::HashSet<i64> = fts_ids.into_iter().collect();

    let mut hits: Vec<Hit> = Vec::new();
    for id in all_ids {
        let Some(mut h) = details.remove(&id) else {
            continue;
        };
        h.vec_score = vec_scores.get(&id).copied();
        let pass_vec = h.vec_score.map(|s| s >= opts.threshold).unwrap_or(false);
        let pass_fts = fts_set.contains(&id) && fts_hit_plausible(&h, &terms);
        if !(pass_vec || pass_fts) {
            continue;
        }
        h.score = fused[&id] * recency_boost(&h.kind, h.created_at, now);
        hits.push(h);
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 每条记录最多保留 N 个切片
    let mut per_item: HashMap<i64, usize> = HashMap::new();
    hits.retain(|h| {
        let c = per_item.entry(h.item_id).or_insert(0);
        *c += 1;
        *c <= MAX_CHUNKS_PER_ITEM
    });
    // 重排序只处理少量候选:取最终条数的 2 倍(至少 12、至多 24)
    let cand = (opts.top_n * 2).clamp(12, 24);
    hits.truncate(cand);
    if rerank_applicable(core, hits.len()) {
        on_stage("rerank");
    }
    rerank_hits(core, query, &mut hits).await;
    hits.truncate(opts.top_n);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf对两路都靠前的结果加分更多() {
        let m = rrf_fuse(&[vec![1, 2, 3], vec![3, 1, 4]]);
        assert!(m[&1] > m[&2]);
        assert!(m[&1] > m[&4]);
        assert!(m[&3] > m[&2]);
    }

    #[test]
    fn 重排序概率为主时效仅作平局项() {
        // 相关性差距大时,时效加权不能翻盘
        assert!(final_score(0.9, 1.0) > final_score(0.5, 1.2));
        // 相关性相同,更新的略靠前
        assert!(final_score(0.5, 1.2) > final_score(0.5, 1.0));
        assert!(final_score(0.5, 1.2) - final_score(0.5, 1.0) <= 0.03 + 1e-6);
    }

    #[test]
    fn 新记录加权文档不衰减() {
        let day = 86_400_000;
        let now = 1000 * day;
        assert!(recency_boost("note", now, now) > recency_boost("note", now - 365 * day, now));
        assert_eq!(recency_boost("document", 0, now), 1.0);
        assert!(recency_boost("note", now, now) <= 1.2 + 1e-5);
        // 未来时间不应产生超过上限的加权
        assert!(recency_boost("note", now + day, now) <= 1.2 + 1e-5);
    }
}
