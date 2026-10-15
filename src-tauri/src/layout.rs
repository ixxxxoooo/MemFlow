// @author ygw
//! 版面还原:把 OCR 行(带坐标)或 PDF 文本层的“视觉行”整理成结构化 Markdown。
//!
//! 纯逻辑模块,不依赖平台,便于单元测试:
//! - `lines_to_markdown`:OCR 行 → 阅读顺序、段落合并、标题(按字号)、表格(按坐标对齐)
//! - `reflow_text_layer`:文本层视觉行 → 段落重排、标题识别、去页码
//! - `is_scanned`:判断一页是否缺少文本层(需要 OCR)

/// OCR 识别出的一行(坐标已归一化到 0~1,原点在页面左下角,与 Vision 一致)
#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    pub text: String,
    /// 左边界
    pub x: f32,
    /// 下边界
    pub y: f32,
    /// 宽
    pub w: f32,
    /// 高
    pub h: f32,
}

/// 页面有效字符少于该值,认为没有可用的文本层(扫描页)
pub const SCANNED_MIN_CHARS: usize = 25;

/// 判断一页的文本层是否可用
///
/// 参数: text 文本层内容
/// 返回: 需要 OCR 时为 true
pub fn is_scanned(text: &str) -> bool {
    text.chars().filter(|c| !c.is_whitespace()).count() < SCANNED_MIN_CHARS
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// 拼接两段文本:中日韩直接相连,拉丁文字之间补一个空格,英文断词连字符直接接上
fn join_text(a: &str, b: &str) -> String {
    let (Some(last), Some(first)) = (a.chars().last(), b.chars().next()) else {
        return format!("{a}{b}");
    };
    if is_cjk(last) || is_cjk(first) {
        return format!("{a}{b}");
    }
    if last == '-' && a.chars().rev().nth(1).is_some_and(|c| c.is_alphabetic()) {
        return format!("{}{}", &a[..a.len() - 1], b);
    }
    format!("{a} {b}")
}

/// 标题行不应以逗号/句号等结尾
fn ends_with_punct(s: &str) -> bool {
    s.trim_end().chars().last().is_some_and(|c| {
        matches!(
            c,
            '。' | '，' | ',' | '；' | ';' | '.' | '!' | '?' | '！' | '？' | '、' | '：' | ':'
        )
    })
}

/// 根据常见编号/关键词识别标题级别(1~4),不是标题返回 None
///
/// 参数: line 单行文本
/// 返回: 标题级别
pub fn heading_level(line: &str) -> Option<usize> {
    let t = line.trim();
    let n = t.chars().count();
    if t.is_empty() || n > 40 || ends_with_punct(t) {
        return None;
    }
    // 已经是 Markdown 标题
    if t.starts_with('#') {
        return None;
    }
    let cn_num = "一二三四五六七八九十百零〇两";
    let chars: Vec<char> = t.chars().collect();
    // 第X章/篇/部/节/条
    if chars[0] == '第' {
        let mut i = 1;
        while i < chars.len() && (cn_num.contains(chars[i]) || chars[i].is_ascii_digit()) {
            i += 1;
        }
        if i > 1 && i < chars.len() {
            return match chars[i] {
                '章' | '篇' | '部' => Some(1),
                '节' | '条' => Some(2),
                _ => None,
            };
        }
    }
    // 一、二、三、
    if cn_num.contains(chars[0]) {
        let mut i = 0;
        while i < chars.len() && cn_num.contains(chars[i]) {
            i += 1;
        }
        if i < chars.len() && chars[i] == '、' {
            return Some(2);
        }
    }
    // (一) / （一）
    if matches!(chars[0], '(' | '（') && chars.len() > 2 && cn_num.contains(chars[1]) {
        if let Some(p) = chars.iter().position(|c| matches!(c, ')' | '）')) {
            if p <= 4 {
                return Some(3);
            }
        }
    }
    // 1.2.3 标题 / 1.2 标题
    let mut j = 0;
    let mut groups = 0;
    loop {
        let start = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j == start {
            break;
        }
        groups += 1;
        if j < chars.len()
            && chars[j] == '.'
            && j + 1 < chars.len()
            && chars[j + 1].is_ascii_digit()
        {
            j += 1;
        } else {
            break;
        }
    }
    if groups >= 2
        && j < chars.len()
        && (chars[j] == ' '
            || chars[j] == '\u{3000}'
            || is_cjk(chars[j])
            || chars[j].is_alphabetic())
    {
        return Some(groups.min(4));
    }
    // “1 概述”“1. 概述”:数字 + 短标题(不含句号等标点由上面保证)
    if groups == 1 && j < chars.len() {
        let rest_start = if chars[j] == '.' { j + 1 } else { j };
        if rest_start < chars.len()
            && (chars[rest_start] == ' ' || chars[rest_start] == '\u{3000}')
            && n <= 25
        {
            let rest = t[t
                .char_indices()
                .nth(rest_start)
                .map(|(i, _)| i)
                .unwrap_or(0)..]
                .trim();
            if rest
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || is_cjk(c))
            {
                return Some(1);
            }
        }
    }
    // 常见一级标题关键词
    let key = t.trim_matches(|c: char| c.is_whitespace() || c == ' ');
    let lower = key.to_lowercase();
    if matches!(
        key,
        "摘要"
            | "目录"
            | "前言"
            | "引言"
            | "绪论"
            | "结论"
            | "总结"
            | "参考文献"
            | "附录"
            | "致谢"
    ) || matches!(
        lower.as_str(),
        "abstract" | "introduction" | "conclusion" | "references" | "appendix" | "contents"
    ) || lower.starts_with("chapter ")
    {
        return Some(1);
    }
    None
}

/// 是否为应丢弃的页码行(纯数字、`- 3 -`、`第 3 页`、`3 / 20` 等)
fn is_page_number(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t.chars().count() > 12 {
        return false;
    }
    let stripped: String = t
        .chars()
        .filter(|c| !matches!(c, '-' | '—' | '–' | ' ' | '/' | '第' | '页' | '共'))
        .collect();
    !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit())
}

/// 文本层重排:把按视觉行断开的文本重新合并为段落,识别标题并去掉页码
///
/// 参数: text 一页的文本层(PDFKit 输出,每个视觉行一行)
/// 返回: 以空行分隔段落、`#` 标记标题的 Markdown 文本
pub fn reflow_text_layer(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !is_page_number(l))
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    // 版心宽度:取视觉行字数的 90 分位,短于其一定比例的行视为段末
    let mut lens: Vec<usize> = lines.iter().map(|l| l.chars().count()).collect();
    lens.sort_unstable();
    let full = lens[(lens.len() * 9 / 10).min(lens.len() - 1)].max(1);

    let mut out: Vec<String> = Vec::new();
    let mut para = String::new();
    let flush = |para: &mut String, out: &mut Vec<String>| {
        if !para.is_empty() {
            out.push(std::mem::take(para));
        }
    };
    for (i, line) in lines.iter().enumerate() {
        if let Some(level) = heading_level(line) {
            flush(&mut para, &mut out);
            out.push(format!("{} {}", "#".repeat(level), line));
            continue;
        }
        if para.is_empty() {
            para.push_str(line);
        } else {
            para = join_text(&para, line);
        }
        let short = line.chars().count() * 100 < full * 55;
        let next_is_heading = lines.get(i + 1).is_some_and(|n| heading_level(n).is_some());
        // 行明显短于版心(段末)或下一行是标题时,结束当前段落
        if short || next_is_heading {
            flush(&mut para, &mut out);
        }
    }
    flush(&mut para, &mut out);
    out.join("\n\n")
}

/// 一个视觉行:同一水平线上的若干 OCR 文本块
struct Row {
    cells: Vec<OcrLine>,
    /// 行中心(自顶向下,0 在页面顶部)
    cy: f32,
    top: f32,
    bottom: f32,
    h: f32,
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// 把 OCR 文本块聚成视觉行(按行中心对齐),行内按 x 排序
fn group_rows(lines: &[OcrLine], med_h: f32) -> Vec<Row> {
    let mut items: Vec<(f32, &OcrLine)> = lines
        .iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| (1.0 - (l.y + l.h / 2.0), l))
        .collect();
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let tol = (med_h * 0.55).max(0.004);
    let mut rows: Vec<Row> = Vec::new();
    for (cy, l) in items {
        match rows.last_mut() {
            Some(r) if (cy - r.cy).abs() <= tol => {
                r.cells.push(l.clone());
                let n = r.cells.len() as f32;
                r.cy = (r.cy * (n - 1.0) + cy) / n;
                r.top = r.top.min(1.0 - (l.y + l.h));
                r.bottom = r.bottom.max(1.0 - l.y);
                r.h = r.h.max(l.h);
            }
            _ => rows.push(Row {
                cells: vec![l.clone()],
                cy,
                top: 1.0 - (l.y + l.h),
                bottom: 1.0 - l.y,
                h: l.h,
            }),
        }
    }
    for r in &mut rows {
        r.cells.sort_by(|a, b| a.x.total_cmp(&b.x));
    }
    rows
}

/// 行内相邻文本块之间的最小间隙(超过它才算“不同的单元格”)
fn cell_gap(row: &Row, med_h: f32) -> bool {
    row.cells.len() >= 2
        && row
            .cells
            .windows(2)
            .any(|w| w[1].x - (w[0].x + w[0].w) > med_h * 1.2)
}

/// 把若干行组装成 Markdown 表格;列位置由所有单元格左边界聚类得到
fn build_table(rows: &[&Row]) -> Option<String> {
    let mut xs: Vec<f32> = rows
        .iter()
        .flat_map(|r| r.cells.iter().map(|c| c.x))
        .collect();
    xs.sort_by(|a, b| a.total_cmp(b));
    let mut cols: Vec<f32> = Vec::new();
    for x in xs {
        match cols.last() {
            Some(&c) if x - c < 0.035 => {}
            _ => cols.push(x),
        }
    }
    if cols.len() < 2 || cols.len() > 12 {
        return None;
    }
    let mut grid: Vec<Vec<String>> = Vec::new();
    for r in rows {
        let mut cells = vec![String::new(); cols.len()];
        for c in &r.cells {
            let idx = cols
                .iter()
                .enumerate()
                .min_by(|a, b| (a.1 - c.x).abs().total_cmp(&(b.1 - c.x).abs()))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let t = c.text.trim().replace('|', "\\|");
            cells[idx] = if cells[idx].is_empty() {
                t
            } else {
                join_text(&cells[idx], &t)
            };
        }
        grid.push(cells);
    }
    let mut md = String::new();
    let header = &grid[0];
    md.push_str(&format!("| {} |\n", header.join(" | ")));
    md.push_str(&format!("|{}|\n", vec![" --- "; cols.len()].join("|")));
    for row in &grid[1..] {
        md.push_str(&format!("| {} |\n", row.join(" | ")));
    }
    Some(md.trim_end().to_string())
}

/// OCR 行 → Markdown:阅读顺序、标题、段落合并、表格
///
/// 参数: lines 一页的 OCR 结果
/// 返回: Markdown 文本(段落以空行分隔)
pub fn lines_to_markdown(lines: &[OcrLine]) -> String {
    if lines.iter().all(|l| l.text.trim().is_empty()) {
        return String::new();
    }
    let mut hs: Vec<f32> = lines.iter().map(|l| l.h).filter(|h| *h > 0.0).collect();
    let med_h = median(&mut hs).max(0.005);
    let rows = group_rows(lines, med_h);

    let mut blocks: Vec<String> = Vec::new();
    let mut para = String::new();
    let mut prev_bottom: Option<f32> = None;
    let flush = |para: &mut String, blocks: &mut Vec<String>| {
        if !para.is_empty() {
            blocks.push(std::mem::take(para));
        }
    };

    let mut i = 0;
    while i < rows.len() {
        // 表格:连续两行及以上都存在多列
        if cell_gap(&rows[i], med_h) {
            let mut j = i;
            while j < rows.len() && cell_gap(&rows[j], med_h) {
                j += 1;
            }
            if j - i >= 2 {
                let group: Vec<&Row> = rows[i..j].iter().collect();
                if let Some(t) = build_table(&group) {
                    flush(&mut para, &mut blocks);
                    blocks.push(t);
                    prev_bottom = Some(rows[j - 1].bottom);
                    i = j;
                    continue;
                }
            }
        }
        let row = &rows[i];
        let text = row
            .cells
            .iter()
            .map(|c| c.text.trim().to_string())
            .reduce(|a, b| join_text(&a, &b))
            .unwrap_or_default();
        // 标题:明显更大的字号或编号模式,且是短行
        let size_level = if row.h > med_h * 1.7 {
            Some(1)
        } else if row.h > med_h * 1.3 {
            Some(2)
        } else {
            None
        };
        let level = if text.chars().count() <= 40 && !ends_with_punct(&text) {
            size_level.or_else(|| heading_level(&text))
        } else {
            None
        };
        if let Some(l) = level {
            flush(&mut para, &mut blocks);
            blocks.push(format!("{} {}", "#".repeat(l), text));
            prev_bottom = Some(row.bottom);
            i += 1;
            continue;
        }
        // 段落:与上一行间距过大则另起一段
        let new_para = prev_bottom.is_some_and(|b| row.top - b > med_h * 0.9);
        if new_para {
            flush(&mut para, &mut blocks);
        }
        if para.is_empty() {
            para = text;
        } else {
            para = join_text(&para, &text);
        }
        prev_bottom = Some(row.bottom);
        i += 1;
    }
    flush(&mut para, &mut blocks);
    blocks.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: f32, top: f32, w: f32, h: f32) -> OcrLine {
        // top 为自顶向下的位置,转换成 Vision 的左下原点
        OcrLine {
            text: text.into(),
            x,
            y: 1.0 - top - h,
            w,
            h,
        }
    }

    #[test]
    fn 扫描页判断() {
        assert!(is_scanned("  \n 12 \n"));
        assert!(!is_scanned(&"这是一页有文本层的内容。".repeat(5)));
    }

    #[test]
    fn 标题级别识别() {
        assert_eq!(heading_level("第一章 总则"), Some(1));
        assert_eq!(heading_level("第三节 适用范围"), Some(2));
        assert_eq!(heading_level("二、项目背景"), Some(2));
        assert_eq!(heading_level("（三）实施步骤"), Some(3));
        assert_eq!(heading_level("1.2 研究方法"), Some(2));
        assert_eq!(heading_level("2.3.1 数据采集"), Some(3));
        assert_eq!(heading_level("3 系统设计"), Some(1));
        assert_eq!(heading_level("摘要"), Some(1));
        assert_eq!(heading_level("Abstract"), Some(1));
        // 正文句子、以标点结尾、过长都不是标题
        assert_eq!(heading_level("今天天气很好。"), None);
        assert_eq!(heading_level("第一章 总则,"), None);
        assert_eq!(heading_level(&"很长的一行".repeat(10)), None);
        assert_eq!(heading_level("2026年10月"), None);
    }

    #[test]
    fn 页码行被丢弃() {
        for s in ["12", "- 3 -", "第 3 页", "3 / 20", "— 7 —"] {
            assert!(is_page_number(s), "{s}");
        }
        assert!(!is_page_number("第3章"));
        assert!(!is_page_number("2026年"));
    }

    #[test]
    fn 文本层重排合并段落并识别标题() {
        let text = "第一章 总则\n本办法适用于公司全体员工,员工在工作期间应当遵守\n以下规定。\n第二段独立成段,而且比较短。\n12";
        let r = reflow_text_layer(text);
        assert!(r.starts_with("# 第一章 总则"), "{r}");
        assert!(
            r.contains("员工在工作期间应当遵守以下规定。"),
            "视觉行应被合并: {r}"
        );
        assert!(r.contains("\n\n第二段独立成段"), "{r}");
        assert!(!r.contains("\n12"), "页码应被去掉: {r}");
    }

    #[test]
    fn 英文行合并补空格并处理连字符() {
        assert_eq!(join_text("hello", "world"), "hello world");
        assert_eq!(join_text("infor-", "mation"), "information");
        assert_eq!(join_text("你好", "世界"), "你好世界");
        assert_eq!(join_text("版本", "v2"), "版本v2");
    }

    #[test]
    fn ocr行按阅读顺序合并为段落() {
        // 故意乱序传入
        let lines = vec![
            line("第二行内容接着写", 0.1, 0.116, 0.6, 0.02),
            line("第一行内容开头", 0.1, 0.09, 0.6, 0.02),
            line("新的一段在这里", 0.1, 0.20, 0.6, 0.02),
        ];
        let md = lines_to_markdown(&lines);
        assert_eq!(md, "第一行内容开头第二行内容接着写\n\n新的一段在这里");
    }

    #[test]
    fn ocr大字号识别为标题() {
        let lines = vec![
            line("年度报告", 0.3, 0.05, 0.4, 0.05),
            line("这是正文第一句话。", 0.1, 0.15, 0.7, 0.02),
            line("这是正文第二句话。", 0.1, 0.18, 0.7, 0.02),
            line("正文第三行。", 0.1, 0.21, 0.5, 0.02),
        ];
        let md = lines_to_markdown(&lines);
        assert!(md.starts_with("# 年度报告"), "{md}");
        assert!(
            md.contains("这是正文第一句话。这是正文第二句话。正文第三行。"),
            "{md}"
        );
    }

    #[test]
    fn ocr坐标对齐的多列识别为markdown表格() {
        let mut lines = vec![line("以下是预算表", 0.1, 0.08, 0.3, 0.02)];
        for (r, row) in [
            ["项目", "金额", "负责人"],
            ["差旅", "1200", "老王"],
            ["设备", "8800", "小李"],
        ]
        .iter()
        .enumerate()
        {
            let top = 0.2 + r as f32 * 0.04;
            for (c, cell) in row.iter().enumerate() {
                lines.push(line(cell, 0.1 + c as f32 * 0.3, top, 0.1, 0.02));
            }
        }
        lines.push(line("表格结束后的说明文字。", 0.1, 0.45, 0.5, 0.02));
        let md = lines_to_markdown(&lines);
        assert!(md.contains("| 项目 | 金额 | 负责人 |"), "{md}");
        assert!(
            md.contains("| --- | --- | --- |")
                || md.contains("|---|---|---|")
                || md.contains("| --- |"),
            "{md}"
        );
        assert!(md.contains("| 差旅 | 1200 | 老王 |"), "{md}");
        assert!(md.contains("| 设备 | 8800 | 小李 |"), "{md}");
        let first = md.find("以下是预算表").unwrap();
        let table = md.find("| 项目").unwrap();
        let tail = md.find("表格结束").unwrap();
        assert!(
            first < table && table < tail,
            "顺序应为 说明-表格-说明: {md}"
        );
    }

    #[test]
    fn 单个多列行不会误判为表格() {
        let lines = vec![
            line("左侧文字", 0.1, 0.1, 0.1, 0.02),
            line("右侧文字", 0.7, 0.1, 0.1, 0.02),
            line("下一行普通正文。", 0.1, 0.2, 0.5, 0.02),
        ];
        let md = lines_to_markdown(&lines);
        assert!(!md.contains('|'), "{md}");
    }

    #[test]
    fn 空输入返回空串() {
        assert_eq!(lines_to_markdown(&[]), "");
        assert_eq!(reflow_text_layer("  \n"), "");
    }
}
