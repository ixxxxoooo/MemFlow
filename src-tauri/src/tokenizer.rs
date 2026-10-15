// @author ixxxxoooo
//! 中文分词:FTS5 默认分词器无法处理中文,这里用 jieba 预分词后写入/查询。
//!
//! - 索引时使用搜索引擎模式(cut_for_search),召回更全
//! - 查询时使用精确模式(cut),并剔除停用词与标点

use jieba_rs::Jieba;
use std::collections::HashSet;
use std::sync::OnceLock;

static JIEBA: OnceLock<Jieba> = OnceLock::new();

/// 获取全局 jieba 实例(首次调用会加载词典)
fn jieba() -> &'static Jieba {
    JIEBA.get_or_init(Jieba::new)
}

/// 常见停用词(查询时剔除,避免"的/了/吗"之类造成无意义命中)
const STOPWORDS: &[&str] = &[
    "的",
    "了",
    "是",
    "我",
    "你",
    "他",
    "她",
    "它",
    "们",
    "吗",
    "呢",
    "吧",
    "啊",
    "呀",
    "什么",
    "怎么",
    "怎样",
    "如何",
    "哪个",
    "哪些",
    "有没有",
    "有",
    "在",
    "和",
    "与",
    "及",
    "或",
    "也",
    "都",
    "就",
    "还",
    "这",
    "那",
    "这个",
    "那个",
    "一个",
    "一下",
    "请",
    "帮",
    "帮我",
    "告诉",
    "关于",
    "之前",
    "上次",
    "最近",
    "今天",
    "昨天",
    "the",
    "a",
    "an",
    "of",
    "to",
    "is",
    "are",
    "and",
    "or",
    "in",
    "on",
    "for",
    "what",
    "how",
    "do",
    "does",
];

/// 判断分词结果是否应保留(含字母/数字/汉字)
fn is_meaningful(tok: &str) -> bool {
    !tok.trim().is_empty() && tok.chars().any(|c| c.is_alphanumeric())
}

/// 索引分词:返回用空格连接的词序列(小写)
///
/// 参数: text 原文
/// 返回: 空格分隔的 token 串,可直接写入 FTS5
pub fn index_tokens(text: &str) -> String {
    let lowered = text.to_lowercase();
    jieba()
        .cut_for_search(&lowered, true)
        .into_iter()
        .filter(|t| is_meaningful(t))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 查询分词:返回去重、去停用词后的有效关键词
///
/// 参数: query 用户查询
/// 返回: 关键词列表(保持出现顺序)
pub fn query_terms(query: &str) -> Vec<String> {
    let lowered = query.to_lowercase();
    let stop: HashSet<&str> = STOPWORDS.iter().copied().collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in jieba().cut(&lowered, true) {
        let t = t.trim();
        if !is_meaningful(t) || stop.contains(t) {
            continue;
        }
        // 单个字符的纯 ASCII 噪声直接丢弃
        if t.chars().count() == 1 && t.is_ascii() {
            continue;
        }
        if seen.insert(t.to_string()) {
            out.push(t.to_string());
        }
    }
    out
}

/// 把关键词列表构造成 FTS5 的 MATCH 表达式(每个词加引号并用 OR 连接)
///
/// 参数: terms 关键词
/// 返回: MATCH 表达式;关键词为空时返回 None
pub fn fts_match_expr(terms: &[String]) -> Option<String> {
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 索引分词包含中文词() {
        let t = index_tokens("老王说下个月预算会缩减20%");
        assert!(t.contains("预算"), "{t}");
        assert!(t.contains("老王"), "{t}");
    }

    #[test]
    fn 查询分词去掉停用词() {
        let t = query_terms("上次老王说的预算是什么");
        assert!(t.contains(&"预算".to_string()), "{t:?}");
        assert!(!t.contains(&"的".to_string()));
        assert!(!t.contains(&"什么".to_string()));
    }

    #[test]
    fn 匹配表达式转义引号() {
        let e = fts_match_expr(&["a\"b".into(), "预算".into()]).unwrap();
        assert_eq!(e, "\"a\"\"b\" OR \"预算\"");
        assert!(fts_match_expr(&[]).is_none());
    }
}
