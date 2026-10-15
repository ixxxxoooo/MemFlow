// @author ygw
//! 语义分块:按标题 → 段落 → 句子逐级切分,再贪心打包到目标长度,并带上下文重叠。
//!
//! 所有长度均按"字符数"计算,切分点始终落在字符边界上,不会截断中文/emoji。

/// 分块参数
#[derive(Debug, Clone, Copy)]
pub struct ChunkOptions {
    /// 单块目标最大字符数
    pub max_chars: usize,
    /// 相邻块重叠字符数
    pub overlap: usize,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        ChunkOptions {
            max_chars: 600,
            overlap: 80,
        }
    }
}

/// 分块结果
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkPiece {
    /// 块文本
    pub text: String,
    /// 所属标题路径(Markdown 标题),无则为空
    pub heading: String,
    /// 起始页码(从 1 开始),文本不分页时为 None
    pub page: Option<u32>,
}

/// 页边界标记:PDF 抽取时在每一页开头插入单独一行,分块时据此还原页码并剔除。
/// 使用私有区字符,不会与正文冲突,也不是空白字符(不会被文本清洗吞掉)。
pub const PAGE_MARK: char = '\u{E000}';

/// 去掉页边界标记(用于不关心页码的场景,如摘要提取、预览)
///
/// 参数: text 含页标记的文本
/// 返回: 不含标记的文本(标记行整行移除)
pub fn strip_page_marks(text: &str) -> String {
    if !text.contains(PAGE_MARK) {
        return text.to_string();
    }
    text.lines()
        .filter(|l| l.trim() != PAGE_MARK.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 一个待打包的单元(段落/表格)
struct Unit {
    text: String,
    page: Option<u32>,
    table: bool,
}

/// 判断一个段落是否 Markdown 表格(每一行都以 | 开头,至少表头 + 分隔行)
fn is_table(p: &str) -> bool {
    let lines: Vec<&str> = p.lines().collect();
    lines.len() >= 2 && lines.iter().all(|l| l.trim_start().starts_with('|'))
}

/// 把表格切成不超过 max 字符的若干块,每块都重复表头(前两行)
fn split_table(p: &str, max: usize) -> Vec<String> {
    let lines: Vec<&str> = p.lines().collect();
    if p.chars().count() <= max {
        return vec![p.to_string()];
    }
    let head = lines[..2.min(lines.len())].join("\n");
    let head_len = head.chars().count();
    let mut out = Vec::new();
    let mut cur = head.clone();
    let mut cur_len = head_len;
    let mut rows_in_cur = 0;
    for row in lines.iter().skip(2) {
        let rl = row.chars().count() + 1;
        if rows_in_cur > 0 && cur_len + rl > max {
            out.push(std::mem::take(&mut cur));
            cur = head.clone();
            cur_len = head_len;
            rows_in_cur = 0;
        }
        cur.push('\n');
        cur.push_str(row);
        cur_len += rl;
        rows_in_cur += 1;
    }
    if rows_in_cur > 0 {
        out.push(cur);
    }
    out
}

/// 句子结束符
fn is_sentence_end(c: char) -> bool {
    matches!(
        c,
        '。' | '\u{ff01}' | '\u{ff1f}' | '\u{ff1b}' | '!' | '?' | ';' | '…' | '\n'
    )
}

/// 把一个过长的段落按句子切成不超过 max_chars 的小段
fn split_long_paragraph(p: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = p.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    let mut sentence = String::new();
    let mut sent_len = 0usize;

    let flush_sentence = |sentence: &mut String,
                          sent_len: &mut usize,
                          cur: &mut String,
                          cur_len: &mut usize,
                          out: &mut Vec<String>| {
        if sentence.is_empty() {
            return;
        }
        // 单个句子本身超长:硬切
        if *sent_len > max_chars {
            if !cur.is_empty() {
                out.push(std::mem::take(cur));
                *cur_len = 0;
            }
            let cs: Vec<char> = sentence.chars().collect();
            for part in cs.chunks(max_chars) {
                out.push(part.iter().collect());
            }
        } else {
            if *cur_len + *sent_len > max_chars && !cur.is_empty() {
                out.push(std::mem::take(cur));
                *cur_len = 0;
            }
            cur.push_str(sentence);
            *cur_len += *sent_len;
        }
        sentence.clear();
        *sent_len = 0;
    };

    for c in chars {
        sentence.push(c);
        sent_len += 1;
        if is_sentence_end(c) {
            flush_sentence(
                &mut sentence,
                &mut sent_len,
                &mut cur,
                &mut cur_len,
                &mut out,
            );
        }
    }
    flush_sentence(
        &mut sentence,
        &mut sent_len,
        &mut cur,
        &mut cur_len,
        &mut out,
    );
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 取字符串末尾最多 n 个字符(尽量从句子边界开始)
fn tail_overlap(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return String::new();
    }
    let start = chars.len() - n;
    // 在重叠窗口内寻找第一个句子边界,从其后开始,使重叠内容更完整
    let mut begin = start;
    for (i, c) in chars[start..].iter().enumerate() {
        if is_sentence_end(*c) && start + i + 1 < chars.len() {
            begin = start + i + 1;
            break;
        }
    }
    chars[begin..]
        .iter()
        .collect::<String>()
        .trim_start()
        .to_string()
}

/// 判断一行是否 Markdown 标题,返回(级别, 标题文字)
fn parse_heading(line: &str) -> Option<(usize, String)> {
    let t = line.trim_start();
    let level = t.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&level) {
        let rest = &t[level..];
        if rest.starts_with(' ') && !rest.trim().is_empty() {
            return Some((level, rest.trim().to_string()));
        }
    }
    None
}

/// 对文本分块
///
/// 参数: text 全文(可含 `PAGE_MARK` 页标记);opts 分块参数
/// 返回: 分块列表(空文本返回空列表)
pub fn chunk(text: &str, opts: ChunkOptions) -> Vec<ChunkPiece> {
    let text = text.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return vec![];
    }
    let max = opts.max_chars.max(50);
    let overlap = opts.overlap.min(max / 3);
    let page_mark = PAGE_MARK.to_string();
    let paged = text.contains(PAGE_MARK);

    // 1) 按标题切成 section,section 内按空行切段落(记录每段所在页)
    struct Section {
        heading: String,
        paragraphs: Vec<(String, Option<u32>)>,
    }
    let mut sections: Vec<Section> = vec![Section {
        heading: String::new(),
        paragraphs: vec![],
    }];
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    let mut para = String::new();
    let mut in_code = false;
    let mut page: u32 = 0;
    // 段落起始页:段落第一行写入时确定
    let mut para_page: Option<u32> = None;
    let flush_para =
        |para: &mut String, para_page: &mut Option<u32>, sections: &mut Vec<Section>| {
            let p = para.trim();
            if !p.is_empty() {
                sections
                    .last_mut()
                    .unwrap()
                    .paragraphs
                    .push((p.to_string(), *para_page));
            }
            para.clear();
            *para_page = None;
        };
    for line in text.lines() {
        if paged && line.trim() == page_mark {
            flush_para(&mut para, &mut para_page, &mut sections);
            page += 1;
            continue;
        }
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
        }
        if !in_code {
            if let Some((level, title)) = parse_heading(line) {
                flush_para(&mut para, &mut para_page, &mut sections);
                while heading_stack
                    .last()
                    .map(|(l, _)| *l >= level)
                    .unwrap_or(false)
                {
                    heading_stack.pop();
                }
                heading_stack.push((level, title));
                let path = heading_stack
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" > ");
                sections.push(Section {
                    heading: path,
                    paragraphs: vec![],
                });
                // 标题所在页也要记入下一段(标题后紧跟的正文通常同页)
                para_page = (paged && page > 0).then_some(page);
                continue;
            }
            if line.trim().is_empty() {
                flush_para(&mut para, &mut para_page, &mut sections);
                continue;
            }
        }
        if para.is_empty() {
            para_page = (paged && page > 0).then_some(page);
        } else {
            para.push('\n');
        }
        para.push_str(line);
    }
    flush_para(&mut para, &mut para_page, &mut sections);

    // 2) section 内贪心打包;表格独立成块,不与正文合并、不做重叠
    let mut pieces: Vec<ChunkPiece> = Vec::new();
    for sec in sections {
        let mut units: Vec<Unit> = Vec::new();
        for (p, pg) in sec.paragraphs {
            if is_table(&p) {
                units.push(Unit {
                    text: p,
                    page: pg,
                    table: true,
                });
            } else if p.chars().count() > max {
                units.extend(split_long_paragraph(&p, max).into_iter().map(|t| Unit {
                    text: t,
                    page: pg,
                    table: false,
                }));
            } else {
                units.push(Unit {
                    text: p,
                    page: pg,
                    table: false,
                });
            }
        }
        let mut cur = String::new();
        let mut cur_len = 0usize;
        let mut cur_page: Option<u32> = None;
        // cur 中是否已有"真实内容"(不含仅由重叠带入的前缀)
        let mut cur_real = false;
        let mut last_flushed = String::new();
        for u in units {
            if u.table {
                if cur_real && !cur.trim().is_empty() {
                    pieces.push(ChunkPiece {
                        text: std::mem::take(&mut cur),
                        heading: sec.heading.clone(),
                        page: cur_page,
                    });
                }
                cur.clear();
                cur_len = 0;
                cur_real = false;
                last_flushed.clear();
                for t in split_table(&u.text, max) {
                    pieces.push(ChunkPiece {
                        text: t,
                        heading: sec.heading.clone(),
                        page: u.page,
                    });
                }
                continue;
            }
            let ulen = u.text.chars().count();
            if cur_len > 0 && cur_len + 1 + ulen > max {
                last_flushed = cur.clone();
                pieces.push(ChunkPiece {
                    text: std::mem::take(&mut cur),
                    heading: sec.heading.clone(),
                    page: cur_page,
                });
                cur_len = 0;
                cur_real = false;
                // 带入上一块的尾部作为重叠
                let ov = tail_overlap(&last_flushed, overlap);
                if !ov.is_empty() && ov.chars().count() + 1 + ulen <= max {
                    cur_len = ov.chars().count();
                    cur = ov;
                }
            }
            if cur_len > 0 {
                cur.push('\n');
                cur_len += 1;
            }
            if !cur_real {
                cur_page = u.page;
                cur_real = true;
            }
            cur.push_str(&u.text);
            cur_len += ulen;
        }
        if cur_real && !cur.trim().is_empty() && cur != last_flushed {
            pieces.push(ChunkPiece {
                text: cur,
                heading: sec.heading.clone(),
                page: cur_page,
            });
        }
    }

    // 3) 合并过短的碎片(表格块与不同标题的块不合并)
    let mut merged: Vec<ChunkPiece> = Vec::new();
    for p in pieces {
        if let Some(last) = merged.last_mut() {
            if p.text.chars().count() < 30
                && last.heading == p.heading
                && !is_table(&p.text)
                && !is_table(&last.text)
                && last.text.chars().count() + p.text.chars().count() < max
            {
                last.text.push('\n');
                last.text.push_str(&p.text);
                continue;
            }
        }
        merged.push(p);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空文本返回空() {
        assert!(chunk("  \n ", ChunkOptions::default()).is_empty());
    }

    #[test]
    fn 短文本一块() {
        let r = chunk("今天和老王聊了A项目。", ChunkOptions::default());
        assert_eq!(r.len(), 1);
        assert!(r[0].text.contains("老王"));
    }

    #[test]
    fn 长文本被切分且不超长() {
        let sentence = "这是一个用来测试分块的句子,内容足够长一些。";
        let text = sentence.repeat(200);
        let opts = ChunkOptions {
            max_chars: 300,
            overlap: 40,
        };
        let r = chunk(&text, opts);
        assert!(r.len() > 5);
        for p in &r {
            assert!(
                p.text.chars().count() <= 300,
                "块超长: {}",
                p.text.chars().count()
            );
        }
    }

    #[test]
    fn 相邻块存在重叠() {
        let text = (0..60)
            .map(|i| format!("第{i}句话在这里,用于验证重叠逻辑是否生效。"))
            .collect::<String>();
        let r = chunk(
            &text,
            ChunkOptions {
                max_chars: 200,
                overlap: 50,
            },
        );
        assert!(r.len() >= 3);
        let tail = r[0]
            .text
            .chars()
            .rev()
            .take(10)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        assert!(
            r[1].text.contains(&tail),
            "第二块应包含第一块尾部\n尾部:{tail}\n第二块:{}",
            r[1].text
        );
    }

    #[test]
    fn 标题路径被保留() {
        let md = "# 总览\n\n介绍文字。\n\n## 细节\n\n细节内容在这里。\n\n### 子项\n\n子项内容。";
        let r = chunk(md, ChunkOptions::default());
        assert!(r.iter().any(|p| p.heading == "总览 > 细节"), "{r:?}");
        assert!(r.iter().any(|p| p.heading == "总览 > 细节 > 子项"));
    }

    #[test]
    fn 无标点超长文本硬切不会panic() {
        let text = "啊".repeat(2000);
        let r = chunk(
            &text,
            ChunkOptions {
                max_chars: 300,
                overlap: 30,
            },
        );
        assert!(r.len() >= 6);
        assert!(r.iter().all(|p| p.text.chars().count() <= 300));
    }

    #[test]
    fn emoji不被截断() {
        let text = "😀".repeat(1000);
        let r = chunk(
            &text,
            ChunkOptions {
                max_chars: 100,
                overlap: 10,
            },
        );
        assert!(r
            .iter()
            .all(|p| p.text.chars().all(|c| c == '😀' || c == '\n')));
    }

    fn paged(pages: &[&str]) -> String {
        pages
            .iter()
            .map(|p| format!("{PAGE_MARK}\n{p}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn 页码被还原且标记不进入正文() {
        let text = paged(&[
            "第一页的内容在这里,这一页有足够多的文字撑满一个块。",
            "第二页的内容在这里,这一页有足够多的文字撑满一个块。",
            "第三页的内容在这里,这一页有足够多的文字撑满一个块。",
        ]);
        let r = chunk(
            &text,
            ChunkOptions {
                max_chars: 50,
                overlap: 0,
            },
        );
        assert!(r.iter().all(|p| !p.text.contains(PAGE_MARK)), "{r:?}");
        let pages: Vec<Option<u32>> = r.iter().map(|p| p.page).collect();
        assert_eq!(pages.first(), Some(&Some(1)));
        assert_eq!(pages.last(), Some(&Some(3)));
        assert!(r.iter().any(|p| p.page == Some(2)), "{r:?}");
    }

    #[test]
    fn 不分页的文本没有页码() {
        let r = chunk("普通文本内容。", ChunkOptions::default());
        assert!(r.iter().all(|p| p.page.is_none()));
    }

    #[test]
    fn 去除页标记() {
        let t = paged(&["甲", "乙"]);
        assert_eq!(strip_page_marks(&t), "甲\n乙");
        assert_eq!(strip_page_marks("无标记"), "无标记");
    }

    #[test]
    fn 表格独立成块且不与正文混合() {
        let md = "前面的说明文字。\n\n| 项目 | 金额 |\n| --- | --- |\n| 差旅 | 1200 |\n| 设备 | 8800 |\n\n后面的说明文字。";
        let r = chunk(md, ChunkOptions::default());
        let table: Vec<_> = r.iter().filter(|p| p.text.contains("| 差旅")).collect();
        assert_eq!(table.len(), 1, "{r:?}");
        assert!(
            !table[0].text.contains("说明文字"),
            "表格块不应混入正文: {:?}",
            table[0]
        );
        assert!(table[0].text.starts_with("| 项目 | 金额 |"));
    }

    #[test]
    fn 超长表格按行切分并重复表头() {
        let mut md = String::from("| 编号 | 名称 |\n| --- | --- |\n");
        for i in 0..60 {
            md.push_str(&format!("| {i} | 条目名称{i} |\n"));
        }
        let r = chunk(
            &md,
            ChunkOptions {
                max_chars: 200,
                overlap: 20,
            },
        );
        assert!(r.len() >= 3, "{}", r.len());
        for p in &r {
            assert!(
                p.text.starts_with("| 编号 | 名称 |\n| --- | --- |"),
                "每块都应带表头: {}",
                p.text
            );
            assert!(p.text.chars().count() <= 200);
        }
        // 所有行都保留
        let total_rows: usize = r.iter().map(|p| p.text.lines().count() - 2).sum();
        assert_eq!(total_rows, 60);
    }
}
