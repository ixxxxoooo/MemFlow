// @author ygw
//! AI 问答编排:查询改写 → 混合检索 → 带编号资料组装 → 流式生成 → 引用校验 → 落库。
//!
//! 防幻觉策略:
//! 1. 检索不到相关资料时直接拒答,不调用 LLM
//! 2. 系统提示词要求仅依据资料并用 [n] 标注来源
//! 3. 回答完成后校验角标,移除资料中不存在的编号

use crate::core::{CancelToken, Core};
use crate::error::{AppError, Result};
use crate::ollama::{strip_code_fence, ChatMessage};
use crate::prompts;
use crate::repo;
use crate::retrieval::{self, Hit, SearchOptions};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::Arc;

/// 单条资料最大字符数
const MAX_SOURCE_CHARS: usize = 900;
/// 资料总字符预算
const MAX_CONTEXT_CHARS: usize = 6500;
/// 带入的历史消息条数
const HISTORY_MESSAGES: usize = 6;
/// 单条历史消息最大字符数
const HISTORY_MSG_CHARS: usize = 1200;
/// 无资料时的固定回复
pub const NO_RESULT_ANSWER: &str =
    "记录中没有找到与这个问题相关的内容。你可以换个说法再问,或先把相关信息记录/导入进来。";

/// 推送给前端的问答事件
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum ChatEvent {
    /// 会话信息
    #[serde(rename_all = "camelCase")]
    Session { session_id: i64 },
    /// 检索到的资料(编号从 1 开始)
    Sources { sources: Vec<SourceView> },
    /// 文本片段
    Token { text: String },
    /// 完成(answer 为校验后的最终文本)
    #[serde(rename_all = "camelCase")]
    Done {
        message_id: i64,
        answer: String,
        stopped: bool,
    },
}

/// 资料视图
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub n: usize,
    pub chunk_id: i64,
    pub item_id: i64,
    pub kind: String,
    pub title: Option<String>,
    pub text: String,
    pub heading: Option<String>,
    pub created_at: i64,
    pub attachment_path: Option<String>,
    pub score: f32,
}

/// 把检索命中转换为带编号的资料视图
///
/// 参数: hits 命中列表
/// 返回: 资料视图(编号从 1 开始)
pub fn to_sources(hits: &[Hit]) -> Vec<SourceView> {
    hits.iter()
        .enumerate()
        .map(|(i, h)| SourceView {
            n: i + 1,
            chunk_id: h.chunk_id,
            item_id: h.item_id,
            kind: h.kind.clone(),
            title: h.title.clone(),
            text: h.text.clone(),
            heading: h.heading.clone(),
            created_at: h.created_at,
            attachment_path: h.attachment_path.clone(),
            score: h.score,
        })
        .collect()
}

/// 毫秒时间戳转本地日期
fn date_of(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// 组装带编号的资料上下文(受总字符预算约束)
///
/// 参数: hits 命中
/// 返回: (上下文文本, 实际纳入的命中数)
pub fn build_context(hits: &[Hit]) -> (String, usize) {
    let mut out = String::new();
    let mut used = 0usize;
    let mut count = 0usize;
    for (i, h) in hits.iter().enumerate() {
        let body: String = h.text.chars().take(MAX_SOURCE_CHARS).collect();
        let head = match h.kind.as_str() {
            "document" => {
                let mut s = format!(
                    "[{}] 文档《{}》",
                    i + 1,
                    h.title.clone().unwrap_or_default()
                );
                if let Some(hd) = &h.heading {
                    s.push_str(&format!(" 章节:{hd}"));
                }
                if let Some(ds) = h.doc_summary.as_ref().filter(|x| !x.is_empty()) {
                    s.push_str(&format!("\n文档摘要:{ds}"));
                }
                s
            }
            "image" => format!(
                "[{}] 图片({}) 记录于 {}",
                i + 1,
                h.title.clone().unwrap_or_default(),
                date_of(h.created_at)
            ),
            _ => {
                let mut s = format!("[{}] 记录于 {}", i + 1, date_of(h.created_at));
                if !h.tags.is_empty() {
                    s.push_str(&format!(" 标签:{}", h.tags.join("、")));
                }
                s
            }
        };
        let block = format!("{head}\n{body}\n\n");
        if used + block.chars().count() > MAX_CONTEXT_CHARS && count > 0 {
            break;
        }
        used += block.chars().count();
        out.push_str(&block);
        count += 1;
    }
    (out, count)
}

/// 校验并清理回答中的引用角标
///
/// 参数: answer 模型原文;max_n 资料总数
/// 返回: (清理后的文本, 实际被引用的编号集合)。代码块内内容保持不变
pub fn sanitize_citations(answer: &str, max_n: usize) -> (String, BTreeSet<usize>) {
    let re = Regex::new(r"\[(\d{1,3})\]").expect("正则非法");
    let mut cited = BTreeSet::new();
    let mut out = String::with_capacity(answer.len());
    for (i, seg) in answer.split("```").enumerate() {
        if i > 0 {
            out.push_str("```");
        }
        if i % 2 == 1 {
            out.push_str(seg); // 代码块原样保留(Mermaid 里可能有 A[1] 之类写法)
            continue;
        }
        let cleaned = re.replace_all(seg, |caps: &regex::Captures| {
            let n: usize = caps[1].parse().unwrap_or(0);
            if n >= 1 && n <= max_n {
                cited.insert(n);
                caps[0].to_string()
            } else {
                String::new()
            }
        });
        out.push_str(&cleaned);
    }
    (out, cited)
}

/// 把历史消息压缩为发给模型的消息(去掉 Mermaid 代码块以节省上下文)
fn history_messages(msgs: &[repo::MessageView]) -> Vec<ChatMessage> {
    let fence = Regex::new(r"(?s)```mermaid.*?```").expect("正则非法");
    msgs.iter()
        .rev()
        .take(HISTORY_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| {
            let c = fence.replace_all(&m.content, "[已生成流程图]").to_string();
            let c: String = c.chars().take(HISTORY_MSG_CHARS).collect();
            ChatMessage::new(&m.role, c)
        })
        .collect()
}

/// 改写追问为独立检索问句,失败则回退原问题
async fn rewrite_query(core: &Core, history: &[ChatMessage], question: &str) -> String {
    let hist_text = history
        .iter()
        .map(|m| {
            format!(
                "{}:{}",
                if m.role == "user" { "用户" } else { "助手" },
                m.content.chars().take(300).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let msgs = prompts::rewrite_messages(&hist_text, question);
    match core
        .llm()
        .chat(&core.text_model(), &msgs, &core.gen_opts(0.0), None)
        .await
    {
        Ok(t) => {
            let t = t.trim().trim_matches('"').to_string();
            if t.is_empty() || t.chars().count() > 300 {
                question.to_string()
            } else {
                t
            }
        }
        Err(e) => {
            tracing::warn!("查询改写失败,使用原问题: {e}");
            question.to_string()
        }
    }
}

/// 执行一次问答
///
/// 参数: core 核心;session_id 会话(None 则新建);question 问题;emit 事件回调
/// 返回: 完成返回 Ok;LLM 不可用等错误通过 Err 返回(调用方负责展示)
pub async fn ask(
    core: &Arc<Core>,
    session_id: Option<i64>,
    question: &str,
    emit: &(dyn Fn(ChatEvent) + Send + Sync),
) -> Result<()> {
    let q = question.trim();
    if q.is_empty() {
        return Err(AppError::Invalid("问题不能为空".into()));
    }
    if q.chars().count() > 4000 {
        return Err(AppError::Invalid("问题过长(上限 4000 字)".into()));
    }
    let _guard = core.chat_guard();
    let token = CancelToken::default();
    // 新问答会取消仍在进行的旧问答
    if let Some(old) = core.chat_cancel.lock().unwrap().replace(token.clone()) {
        old.cancel();
    }
    let settings = core.settings();

    // 会话与历史
    let sid = match session_id {
        Some(id) => id,
        None => {
            let title: String = q.chars().take(24).collect();
            core.db.with(|c| repo::create_session(c, &title))?
        }
    };
    let prior = core.db.with(|c| repo::list_messages(c, sid))?;
    core.db
        .with(|c| repo::add_message(c, sid, "user", q, None).map(|_| ()))?;
    emit(ChatEvent::Session { session_id: sid });
    let history = history_messages(&prior);

    // 检索
    core.prepare_text_model().await;
    let search_q = if settings.query_rewrite && !history.is_empty() {
        rewrite_query(core, &history, q).await
    } else {
        q.to_string()
    };
    let hits = retrieval::hybrid_search(
        core,
        &search_q,
        &SearchOptions {
            top_n: settings.top_k,
            threshold: settings.score_threshold,
        },
    )
    .await?;

    // 无资料:拒答
    if hits.is_empty() {
        let id = core.db.with(|c| {
            repo::add_message(
                c,
                sid,
                "assistant",
                NO_RESULT_ANSWER,
                Some(&serde_json::json!([])),
            )
        })?;
        emit(ChatEvent::Sources { sources: vec![] });
        emit(ChatEvent::Token {
            text: NO_RESULT_ANSWER.into(),
        });
        emit(ChatEvent::Done {
            message_id: id,
            answer: NO_RESULT_ANSWER.into(),
            stopped: false,
        });
        return Ok(());
    }

    let (context, used) = build_context(&hits);
    let sources = to_sources(&hits[..used]);
    emit(ChatEvent::Sources {
        sources: sources.clone(),
    });

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut messages = vec![ChatMessage::new(
        "system",
        prompts::chat_system_prompt(&today, &context, prompts::wants_diagram(q)),
    )];
    messages.extend(history);
    messages.push(ChatMessage::new("user", q));

    let llm = core.llm();
    let mut partial = String::new();
    let result = llm
        .chat_stream(
            &core.text_model(),
            &messages,
            &core.gen_opts(0.3),
            &token.0,
            |t| {
                partial.push_str(t);
                emit(ChatEvent::Token {
                    text: t.to_string(),
                });
            },
        )
        .await;
    let sources_json = serde_json::to_value(&sources)?;
    match result {
        Ok(full) => {
            let (final_text, _) = sanitize_citations(&full, sources.len());
            let id = core.db.with(|c| {
                repo::add_message(c, sid, "assistant", &final_text, Some(&sources_json))
            })?;
            emit(ChatEvent::Done {
                message_id: id,
                answer: final_text,
                stopped: false,
            });
            Ok(())
        }
        Err(AppError::Cancelled) => {
            // 保存已生成的部分内容并标注已停止
            let text = if partial.trim().is_empty() {
                "(已停止生成)".to_string()
            } else {
                format!("{}\n\n(已停止生成)", partial.trim_end())
            };
            let id = core
                .db
                .with(|c| repo::add_message(c, sid, "assistant", &text, Some(&sources_json)))?;
            emit(ChatEvent::Done {
                message_id: id,
                answer: text,
                stopped: true,
            });
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// 修复 Mermaid 语法
///
/// 参数: core 核心;code 出错代码;error 报错信息
/// 返回: 修复后的代码
pub async fn fix_mermaid(core: &Arc<Core>, code: &str, error: &str) -> Result<String> {
    let code: String = code.chars().take(6000).collect();
    let error: String = error.chars().take(600).collect();
    let _guard = core.chat_guard();
    core.prepare_text_model().await;
    let msgs = prompts::mermaid_fix_messages(&code, &error);
    let text = core
        .llm()
        .chat(&core.text_model(), &msgs, &core.gen_opts(0.1), None)
        .await?;
    let fixed = strip_code_fence(&text).trim().to_string();
    // 兼容模型把 mermaid 语言标记写进首行的情况
    let fixed = fixed
        .strip_prefix("mermaid\n")
        .unwrap_or(&fixed)
        .trim()
        .to_string();
    if fixed.is_empty() {
        return Err(AppError::Parse("模型没有返回修复结果".into()));
    }
    Ok(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(kind: &str, text: &str) -> Hit {
        Hit {
            chunk_id: 1,
            item_id: 1,
            kind: kind.into(),
            title: Some("标题".into()),
            text: text.into(),
            heading: None,
            doc_summary: None,
            summary: None,
            tags: vec!["项目A".into()],
            created_at: 0,
            attachment_path: None,
            score: 1.0,
            vec_score: None,
        }
    }

    #[test]
    fn 非法角标被移除合法保留() {
        let (t, cited) = sanitize_citations("预算缩减20% [1][5],另见 [2]。", 3);
        assert_eq!(t, "预算缩减20% [1],另见 [2]。");
        assert_eq!(cited.into_iter().collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn 代码块内的方括号不被修改() {
        let ans = "流程如下 [1]\n```mermaid\nflowchart TD\nA[9] --> B[1]\n```\n结束 [7]";
        let (t, _) = sanitize_citations(ans, 2);
        assert!(t.contains("A[9] --> B[1]"));
        assert!(!t.contains("[7]"));
        assert!(t.contains("[1]\n```mermaid"));
    }

    #[test]
    fn 零号角标非法() {
        let (t, c) = sanitize_citations("错误 [0]", 5);
        assert_eq!(t, "错误 ");
        assert!(c.is_empty());
    }

    #[test]
    fn 上下文受预算限制且带编号() {
        let hits: Vec<Hit> = (0..30).map(|_| hit("note", &"字".repeat(800))).collect();
        let (ctx, used) = build_context(&hits);
        assert!((1..30).contains(&used));
        assert!(ctx.starts_with("[1] 记录于"));
        assert!(ctx.chars().count() <= MAX_CONTEXT_CHARS + 1000);
        let (ctx, _) = build_context(&[hit("document", "正文")]);
        assert!(ctx.contains("文档《标题》"));
    }

    #[test]
    fn 历史消息去除图表() {
        let msgs = vec![repo::MessageView {
            id: 1,
            role: "assistant".into(),
            content: "说明\n```mermaid\nflowchart TD\nA-->B\n```".into(),
            sources: None,
            created_at: 0,
        }];
        let h = history_messages(&msgs);
        assert!(h[0].content.contains("[已生成流程图]"));
        assert!(!h[0].content.contains("flowchart"));
    }
}
