// @author ygw
//! AI 整理:把零散的碎片记忆整理成结构清晰的笔记。
//!
//! 两种方式(都是"先出方案、用户确认后才写入"):
//! - 单条整理:模型给出整理后的标题与正文,用户对比确认后覆盖原文,旧版本进修订历史,可一键撤销
//! - 批量归并:用已有向量(记录中心向量的余弦相似度 + 时间窗口)把相近的碎片聚成组,
//!   每组只调用一次模型生成带 `[来源编号]` 的主题笔记;确认后新建笔记并记录 `merged_from` 关系,原记录保留
//!
//! 安全约束:提示词禁止添加新事实;另外对数字做确定性校验(新增/丢失都会给出警告),
//! 引用编号越界会被移除并提示。

use crate::chat_models;
use crate::core::Core;
use crate::db::now_ms;
use crate::error::{AppError, Result};
use crate::prompts::{self, OrganizeOutput};
use crate::queue;
use crate::repo::{self, NoteBrief};
use chrono::TimeZone;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};

/// 单条整理的原文长度上限(字)
pub const MAX_NOTE_INPUT: usize = 8000;
/// 归并时每组最少/最多的记录数
pub const MIN_GROUP: usize = 3;
pub const MAX_GROUP: usize = 12;
/// 归并时送给模型的总字数上限
pub const MAX_MERGE_CHARS: usize = 24000;
/// 聚类的余弦相似度阈值(bge-m3 下同主题碎片通常在 0.7 以上)
pub const DEFAULT_THRESHOLD: f32 = 0.70;
/// "可整理"提示使用的更严格阈值
pub const HINT_THRESHOLD: f32 = 0.82;
/// 默认回看天数与相似记录的时间窗口(天)
pub const DEFAULT_LOOKBACK_DAYS: i64 = 30;
pub const DEFAULT_WINDOW_DAYS: i64 = 14;
/// 参与聚类的候选上限(O(n²) 比较,保持在可接受的范围)
const MAX_CANDIDATES: usize = 600;
const DAY_MS: i64 = 86_400_000;

/// 单条整理方案
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleProposal {
    pub item_id: i64,
    /// 生成方案时记录的更新时间,确认时用来发现原文已被改动
    pub base_updated_at: i64,
    pub original_title: Option<String>,
    pub original_content: String,
    pub title: String,
    pub content: String,
    /// 模型标出的不确定之处
    pub notes: Vec<String>,
    /// 程序校验出的风险(数字新增/丢失等)
    pub warnings: Vec<String>,
    pub model: String,
    pub cloud: bool,
}

/// 归并方案
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeProposal {
    /// 来源记录 id,顺序即 `[1]`、`[2]`... 的编号
    pub ids: Vec<i64>,
    pub sources: Vec<NoteBrief>,
    pub title: String,
    pub content: String,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
    pub model: String,
    pub cloud: bool,
}

/// 聚类建议的一组
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSuggestion {
    pub ids: Vec<i64>,
    pub items: Vec<NoteBrief>,
    /// 组内两两相似度的平均值
    pub avg_similarity: f32,
}

/// 聚类参数
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroupOptions {
    /// 限定知识库;None 为全部
    pub kb_id: Option<i64>,
    /// 回看天数
    pub lookback_days: Option<i64>,
    /// 同组记录创建时间的最大间隔(天)
    pub window_days: Option<i64>,
    pub threshold: Option<f32>,
    pub min_size: Option<usize>,
    pub max_size: Option<usize>,
}

fn re_cite() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\[(\d{1,3})\]").unwrap())
}

fn re_list_marker() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^\s*\d{1,2}[.、)]\s+").unwrap())
}

fn re_number() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\d+(?:[.,:]\d+)*").unwrap())
}

/// 提取文本里的数字串(忽略列表序号与 `[n]` 引用编号)
fn number_tokens(text: &str) -> BTreeSet<String> {
    let t = re_cite().replace_all(text, "");
    let t = re_list_marker().replace_all(&t, "");
    re_number()
        .find_iter(&t)
        .map(|m| m.as_str().trim_end_matches(['.', ',', ':']).to_string())
        .collect()
}

/// 对比整理前后的数字,返回风险提示(新增了原文没有的数字、丢失了原文的数字)
pub fn number_warnings(original: &str, output: &str) -> Vec<String> {
    let before = number_tokens(original);
    let after = number_tokens(output);
    let added: Vec<&String> = after.difference(&before).collect();
    let missing: Vec<&String> = before.difference(&after).collect();
    let join = |v: &[&String]| {
        v.iter()
            .take(10)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("、")
    };
    let mut out = vec![];
    if !added.is_empty() {
        out.push(format!("整理后出现了原文没有的数字:{}", join(&added)));
    }
    if !missing.is_empty() {
        out.push(format!("整理后丢失了原文中的数字:{}", join(&missing)));
    }
    out
}

/// 移除越界的来源编号;没有任何引用时给出提示
pub fn check_citations(content: &str, n: usize) -> (String, Vec<String>) {
    let mut bad: BTreeSet<usize> = BTreeSet::new();
    let mut any_valid = false;
    let cleaned = re_cite().replace_all(content, |c: &regex::Captures| {
        let k: usize = c[1].parse().unwrap_or(0);
        if (1..=n).contains(&k) {
            any_valid = true;
            c[0].to_string()
        } else {
            bad.insert(k);
            String::new()
        }
    });
    let mut warnings = vec![];
    if !bad.is_empty() {
        let list: Vec<String> = bad.iter().map(|k| format!("[{k}]")).collect();
        warnings.push(format!(
            "模型引用了不存在的来源编号 {},已移除",
            list.join("、")
        ));
    }
    if !any_valid {
        warnings.push("整理结果没有标注来源编号,请对照来源核对".into());
    }
    (cleaned.into_owned(), warnings)
}

/// 毫秒时间戳 → 本地日期时间文案
fn date_label(ms: i64) -> String {
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 调用模型生成整理结果
async fn run_model(
    core: &Arc<Core>,
    model_ref: Option<&str>,
    msgs: Vec<crate::ollama::ChatMessage>,
) -> Result<(OrganizeOutput, chat_models::ResolvedModel)> {
    let model = chat_models::resolve(core, model_ref)?;
    let _guard = core.chat_guard();
    if model.is_local() {
        core.prepare_chat_model(&model.model).await;
    }
    let out: OrganizeOutput = model
        .client
        .chat_json(
            &model.model,
            &msgs,
            &core.gen_opts(0.2),
            &prompts::organize_schema(),
        )
        .await?;
    if out.content.trim().is_empty() {
        return Err(AppError::Parse("模型没有返回整理结果".into()));
    }
    Ok((out, model))
}

fn clean_title(t: &str) -> String {
    t.trim()
        .trim_matches(['"', '“', '”', '《', '》'])
        .chars()
        .take(40)
        .collect()
}

fn clean_notes(notes: Vec<String>) -> Vec<String> {
    notes
        .into_iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .take(10)
        .collect()
}

/// 生成单条整理方案(不写入任何数据)
///
/// 参数: core 核心;item_id 笔记 id;model_ref 使用的对话模型引用(None 为默认)
pub async fn propose_single(
    core: &Arc<Core>,
    item_id: i64,
    model_ref: Option<&str>,
) -> Result<SingleProposal> {
    let item = core
        .db
        .with(|c| repo::get_item(c, item_id))?
        .ok_or_else(|| AppError::Invalid("记录不存在".into()))?;
    if item.kind != "note" {
        return Err(AppError::Invalid("只有文字笔记支持 AI 整理".into()));
    }
    if item.status == "pending" || item.status == "processing" {
        return Err(AppError::Invalid("记录还在处理中,稍后再试".into()));
    }
    let content = item.content.trim().to_string();
    if content.is_empty() {
        return Err(AppError::Invalid("笔记内容为空".into()));
    }
    if content.chars().count() > MAX_NOTE_INPUT {
        return Err(AppError::Invalid(format!(
            "笔记超过 {MAX_NOTE_INPUT} 字,不适合一键整理"
        )));
    }
    let msgs =
        prompts::organize_note_messages(&today(), item.title.as_deref().unwrap_or(""), &content);
    let (out, model) = run_model(core, model_ref, msgs).await?;
    let new_content = out.content.trim().to_string();
    let mut warnings = number_warnings(&content, &new_content);
    if new_content == content {
        warnings.push("整理结果与原文一致,没有可改进之处".into());
    }
    Ok(SingleProposal {
        item_id,
        base_updated_at: item.updated_at,
        original_title: item.title,
        original_content: content,
        title: clean_title(&out.title),
        content: new_content,
        notes: clean_notes(out.notes),
        warnings,
        model: model.model.clone(),
        cloud: !model.is_local(),
    })
}

/// 确认单条整理:旧版本存入修订历史,覆盖原文并重新索引
///
/// 参数: base_updated_at 生成方案时的更新时间,不一致说明原文已变,拒绝覆盖
pub fn apply_single(
    core: &Arc<Core>,
    item_id: i64,
    title: &str,
    content: &str,
    base_updated_at: i64,
) -> Result<()> {
    let content = content.trim();
    if content.is_empty() {
        return Err(AppError::Invalid("整理后的内容不能为空".into()));
    }
    let title = clean_title(title);
    core.db.with_tx(|tx| {
        let item =
            repo::get_item(tx, item_id)?.ok_or_else(|| AppError::Invalid("记录不存在".into()))?;
        if item.kind != "note" {
            return Err(AppError::Invalid("只有文字笔记支持 AI 整理".into()));
        }
        if item.updated_at != base_updated_at {
            return Err(AppError::Invalid("原文已被修改,请重新整理".into()));
        }
        repo::add_revision(
            tx,
            item_id,
            item.title.as_deref(),
            &item.content,
            "ai_organize",
        )?;
        let t = (!title.is_empty()).then_some(title.as_str());
        repo::set_item_text_exact(tx, item_id, t, content)?;
        queue::enqueue_item(tx, item_id, "reindex")?;
        Ok(())
    })?;
    after_change(core, &[item_id]);
    Ok(())
}

/// 撤销最近一次整理,恢复整理前的标题与原文
pub fn undo_single(core: &Arc<Core>, item_id: i64) -> Result<()> {
    core.db.with_tx(|tx| {
        let (title, content) = repo::pop_latest_revision(tx, item_id)?
            .ok_or_else(|| AppError::Invalid("没有可撤销的整理".into()))?;
        repo::set_item_text_exact(tx, item_id, title.as_deref(), &content)?;
        queue::enqueue_item(tx, item_id, "reindex")?;
        Ok(())
    })?;
    after_change(core, &[item_id]);
    Ok(())
}

fn after_change(core: &Arc<Core>, ids: &[i64]) {
    core.wake.notify_one();
    for id in ids {
        core.emit("item-updated", json!({ "id": id }));
    }
    queue::emit_status(core);
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// 并查集
struct Dsu {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            size: vec![1; n],
        }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        let mut c = x;
        while self.parent[c] != r {
            let next = self.parent[c];
            self.parent[c] = r;
            c = next;
        }
        r
    }
    /// 合并两个集合,合并后超过上限则拒绝
    fn union_capped(&mut self, a: usize, b: usize, cap: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb || self.size[ra] + self.size[rb] > cap {
            return false;
        }
        self.parent[rb] = ra;
        self.size[ra] += self.size[rb];
        true
    }
}

/// 把最近的碎片笔记按向量相似度聚成组(不调用模型)
///
/// 做法:取各记录切片向量的中心向量,同库且创建时间相差不超过窗口的两条记录,
/// 余弦相似度达到阈值才连边;按相似度从高到低合并,组大小不超过上限。
pub fn suggest_groups(core: &Core, opts: &GroupOptions) -> Result<Vec<GroupSuggestion>> {
    let lookback = opts.lookback_days.unwrap_or(DEFAULT_LOOKBACK_DAYS).max(1);
    let window = opts.window_days.unwrap_or(DEFAULT_WINDOW_DAYS).max(1) * DAY_MS;
    let threshold = opts.threshold.unwrap_or(DEFAULT_THRESHOLD);
    let min_size = opts.min_size.unwrap_or(MIN_GROUP).max(2);
    let max_size = opts
        .max_size
        .unwrap_or(MAX_GROUP)
        .clamp(min_size, MAX_GROUP);

    let mut cands = core
        .db
        .with_read(|c| repo::organize_candidates(c, opts.kb_id, now_ms() - lookback * DAY_MS))?;
    cands.truncate(MAX_CANDIDATES);
    let ids: Vec<i64> = cands.iter().map(|c| c.id).collect();
    let cents = core.vindex.centroids_of(&ids);
    // 只保留已有向量的记录
    let cands: Vec<NoteBrief> = cands
        .into_iter()
        .filter(|c| cents.contains_key(&c.id))
        .collect();

    let mut edges: Vec<(f32, usize, usize)> = vec![];
    for i in 0..cands.len() {
        let (kb_i, vi) = &cents[&cands[i].id];
        for j in (i + 1)..cands.len() {
            let (kb_j, vj) = &cents[&cands[j].id];
            if kb_i != kb_j || (cands[i].created_at - cands[j].created_at).abs() > window {
                continue;
            }
            let s = cosine(vi, vj);
            if s >= threshold {
                edges.push((s, i, j));
            }
        }
    }
    edges.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut dsu = Dsu::new(cands.len());
    for (_, i, j) in &edges {
        dsu.union_capped(*i, *j, max_size);
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..cands.len() {
        let r = dsu.find(i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<GroupSuggestion> = groups
        .into_values()
        .filter(|g| g.len() >= min_size)
        .map(|mut g| {
            g.sort_by_key(|&i| cands[i].created_at);
            let (mut sum, mut cnt) = (0f32, 0usize);
            for a in 0..g.len() {
                for b in (a + 1)..g.len() {
                    sum += cosine(&cents[&cands[g[a]].id].1, &cents[&cands[g[b]].id].1);
                    cnt += 1;
                }
            }
            GroupSuggestion {
                ids: g.iter().map(|&i| cands[i].id).collect(),
                items: g.iter().map(|&i| cands[i].clone()).collect(),
                avg_similarity: if cnt > 0 { sum / cnt as f32 } else { 0.0 },
            }
        })
        .collect();
    out.sort_by(|a, b| b.avg_similarity.total_cmp(&a.avg_similarity));
    Ok(out)
}

/// "可整理"提示:对给定记录,找出与它高度相似、同样未整理的碎片
///
/// 返回: 记录 id -> 相似记录 id 列表(没有相似记录的不返回)
pub fn peer_hints(core: &Core, ids: &[i64]) -> Result<HashMap<i64, Vec<i64>>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let window = DEFAULT_WINDOW_DAYS * DAY_MS;
    let mut cands = core.db.with_read(|c| {
        repo::organize_candidates(c, None, now_ms() - DEFAULT_LOOKBACK_DAYS * DAY_MS)
    })?;
    cands.truncate(MAX_CANDIDATES);
    let cand_ids: Vec<i64> = cands.iter().map(|c| c.id).collect();
    let cents = core.vindex.centroids_of(&cand_ids);
    let created: HashMap<i64, i64> = cands.iter().map(|c| (c.id, c.created_at)).collect();
    let mut out: HashMap<i64, Vec<i64>> = HashMap::new();
    for id in ids {
        let (Some((kb, v)), Some(t)) = (cents.get(id), created.get(id)) else {
            continue;
        };
        let mut peers: Vec<(f32, i64)> = cands
            .iter()
            .filter(|o| o.id != *id)
            .filter_map(|o| {
                let (okb, ov) = cents.get(&o.id)?;
                if okb != kb || (o.created_at - t).abs() > window {
                    return None;
                }
                let s = cosine(v, ov);
                (s >= HINT_THRESHOLD).then_some((s, o.id))
            })
            .collect();
        if peers.is_empty() {
            continue;
        }
        peers.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.insert(
            *id,
            peers.into_iter().take(MAX_GROUP - 1).map(|p| p.1).collect(),
        );
    }
    Ok(out)
}

/// 校验并按创建时间排序待归并的记录
fn load_sources(core: &Core, ids: &[i64]) -> Result<Vec<(NoteBrief, String)>> {
    let mut uniq: Vec<i64> = vec![];
    for id in ids {
        if !uniq.contains(id) {
            uniq.push(*id);
        }
    }
    if uniq.len() < 2 {
        return Err(AppError::Invalid("请至少选择 2 条笔记".into()));
    }
    if uniq.len() > MAX_GROUP {
        return Err(AppError::Invalid(format!("一次最多归并 {MAX_GROUP} 条")));
    }
    let mut out: Vec<(NoteBrief, String)> = vec![];
    for id in &uniq {
        let item = core
            .db
            .with(|c| repo::get_item(c, *id))?
            .ok_or_else(|| AppError::Invalid(format!("记录 #{id} 不存在")))?;
        if item.kind != "note" {
            return Err(AppError::Invalid("只能归并文字笔记".into()));
        }
        if item.status == "pending" || item.status == "processing" {
            return Err(AppError::Invalid("有记录还在处理中,稍后再试".into()));
        }
        let preview: String = item.content.chars().take(120).collect();
        out.push((
            NoteBrief {
                id: item.id,
                kb_id: item.kb_id,
                title: item.title,
                preview,
                created_at: item.created_at,
            },
            item.content,
        ));
    }
    let kb = out[0].0.kb_id;
    if out.iter().any(|(b, _)| b.kb_id != kb) {
        return Err(AppError::Invalid("所选笔记不在同一个知识库".into()));
    }
    out.sort_by_key(|(b, _)| (b.created_at, b.id));
    Ok(out)
}

/// 生成归并方案(一次模型调用,不写入任何数据)
pub async fn propose_merge(
    core: &Arc<Core>,
    ids: &[i64],
    model_ref: Option<&str>,
) -> Result<MergeProposal> {
    let sources = load_sources(core, ids)?;
    let total: usize = sources.iter().map(|(_, c)| c.chars().count()).sum();
    if total > MAX_MERGE_CHARS {
        return Err(AppError::Invalid(format!(
            "所选内容共 {total} 字,超过 {MAX_MERGE_CHARS} 字上限,请减少条数"
        )));
    }
    let input: Vec<(usize, String, String)> = sources
        .iter()
        .enumerate()
        .map(|(i, (b, c))| (i + 1, date_label(b.created_at), c.trim().to_string()))
        .collect();
    let msgs = prompts::organize_merge_messages(&today(), &input);
    let (out, model) = run_model(core, model_ref, msgs).await?;

    let all_text: String = sources
        .iter()
        .map(|(_, c)| c.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let (content, mut warnings) = check_citations(out.content.trim(), sources.len());
    warnings.extend(number_warnings(&all_text, &content));
    Ok(MergeProposal {
        ids: sources.iter().map(|(b, _)| b.id).collect(),
        sources: sources.into_iter().map(|(b, _)| b).collect(),
        title: clean_title(&out.title),
        content,
        notes: clean_notes(out.notes),
        warnings,
        model: model.model.clone(),
        cloud: !model.is_local(),
    })
}

/// 确认归并:新建主题笔记(正文末尾附来源清单),记录 `merged_from` 关系,原记录保持不变
///
/// 参数: ids 来源记录 id(顺序须与方案里的编号一致);title/content 用户确认(可编辑)后的结果
/// 返回: 新笔记 id
pub fn apply_merge(core: &Arc<Core>, ids: &[i64], title: &str, content: &str) -> Result<i64> {
    let content = content.trim();
    if content.is_empty() {
        return Err(AppError::Invalid("归并后的内容不能为空".into()));
    }
    if ids.len() < 2 || ids.len() > MAX_GROUP {
        return Err(AppError::Invalid("来源记录数量不合法".into()));
    }
    // 来源必须仍然存在且同库;编号以传入顺序为准(与方案一致)
    let mut briefs: Vec<NoteBrief> = vec![];
    for id in ids {
        let b = core
            .db
            .with(|c| repo::note_briefs(c, &[*id]))?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Invalid(format!("来源记录 #{id} 已被删除,请重新归并")))?;
        briefs.push(b);
    }
    let kb_id = briefs[0].kb_id;
    if briefs.iter().any(|b| b.kb_id != kb_id) {
        return Err(AppError::Invalid("所选笔记不在同一个知识库".into()));
    }

    let mut body = content.to_string();
    body.push_str("\n\n---\n来源记录:\n");
    for (i, b) in briefs.iter().enumerate() {
        let name = b
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| b.preview.replace('\n', " ").chars().take(24).collect());
        body.push_str(&format!(
            "[{}] {} · {}(记录 #{})\n",
            i + 1,
            date_label(b.created_at),
            name,
            b.id
        ));
    }
    let title = clean_title(title);
    let new_id = core.db.with_tx(|tx| {
        let id = repo::insert_item(
            tx,
            &repo::NewItem {
                kind: "note".into(),
                title: (!title.is_empty()).then(|| title.clone()),
                content: body.clone(),
                kb_id: Some(kb_id),
                ..Default::default()
            },
        )?;
        for (seq, b) in briefs.iter().enumerate() {
            repo::add_link(
                tx,
                id,
                b.id,
                "merged_from",
                &format!("[{}]", seq + 1),
                seq as i64,
            )?;
        }
        queue::enqueue_item(tx, id, "ingest")?;
        Ok(id)
    })?;
    let mut changed = vec![new_id];
    changed.extend(briefs.iter().map(|b| b.id));
    after_change(core, &changed);
    Ok(new_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 数字校验_新增与丢失都能发现_序号与引用不算() {
        let orig = "预算缩减20%,会议在10月15日,金额3,500元";
        let ok = "1. 预算缩减20%\n2. 会议:10月15日 [1]\n3. 金额3,500元";
        assert!(
            number_warnings(orig, ok).is_empty(),
            "{:?}",
            number_warnings(orig, ok)
        );
        let bad = "预算缩减25%,会议在10月15日";
        let w = number_warnings(orig, bad);
        assert!(w
            .iter()
            .any(|s| s.contains("原文没有的数字") && s.contains("25")));
        assert!(w
            .iter()
            .any(|s| s.contains("丢失") && s.contains("20") && s.contains("3,500")));
    }

    #[test]
    fn 引用校验_越界编号被移除_无引用给提示() {
        let (c, w) = check_citations("预算缩减[1],排期延后[2][5]", 2);
        assert_eq!(c, "预算缩减[1],排期延后[2]");
        assert!(w.iter().any(|s| s.contains("[5]")));
        let (_, w2) = check_citations("没有引用", 3);
        assert!(w2.iter().any(|s| s.contains("没有标注")));
    }

    #[test]
    fn 并查集_合并后超过上限则拒绝() {
        let mut d = Dsu::new(5);
        assert!(d.union_capped(0, 1, 3));
        assert!(d.union_capped(1, 2, 3));
        assert!(!d.union_capped(2, 3, 3), "已有 3 个,不能再并入");
        assert!(d.union_capped(3, 4, 3));
        assert_ne!(d.find(0), d.find(3));
    }
}
