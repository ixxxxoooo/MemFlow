// @author ygw
//! 办公与电子书格式的文本抽取:xlsx / csv / pptx / html / epub / rtf。
//!
//! 不引入重型解析库:这些格式本质是「zip + XML」或纯文本,用 `zip` 与正则即可稳定还原结构
//! (表格→Markdown 表格、标题→`#`、插图→`![](锚点)` 占位),与 docx 的处理方式保持一致。
//! 所有解析都有大小/行数上限,避免超大文件拖垮应用。

use crate::error::{AppError, Result};
use crate::images::{self, EmbeddedImage};
use regex::Regex;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// 单个表格最多导入的数据行数(超出部分截断并提示)
pub const MAX_TABLE_ROWS: usize = 5000;
/// 单个工作表/幻灯片/章节数量上限
const MAX_PARTS: usize = 2000;
/// 读取单个压缩包内文件的字节上限
const ENTRY_LIMIT: u64 = 80 * 1024 * 1024;

// ───────────────────────── 通用工具 ─────────────────────────

/// 解码 HTML/XML 实体(常见命名实体与 `&#N;` / `&#xH;`)
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let re = Regex::new(r"&(#[xX][0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);").expect("正则非法");
    re.replace_all(s, |c: &regex::Captures| {
        let name = &c[1];
        let ch = if let Some(h) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
            u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
        } else if let Some(d) = name.strip_prefix('#') {
            d.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                "ldquo" | "rdquo" => Some('"'),
                "lsquo" | "rsquo" => Some('\''),
                "hellip" => Some('…'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "middot" => Some('·'),
                "copy" => Some('©'),
                _ => None,
            }
        };
        match ch {
            Some(c) => c.to_string(),
            None => c[0].to_string(),
        }
    })
    .into_owned()
}

/// 把字节解码成文本:UTF-8(含 BOM)→ UTF-16(含 BOM)→ GBK(国内 Excel 导出的 csv 常见)
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(b) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(b).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode(&bytes[2..]).0.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE.decode(&bytes[2..]).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}

/// 表格单元格转义:换行压成空格,`|` 转义
fn md_cell(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

/// 行列数据 → Markdown 表格(首行为表头;空表头补「列N」;超过行数上限时截断并提示)
pub fn md_table(rows: &[Vec<String>]) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if rows.is_empty() || width == 0 {
        return String::new();
    }
    let pad = |r: &Vec<String>| -> Vec<String> {
        (0..width)
            .map(|i| md_cell(r.get(i).map(String::as_str).unwrap_or("")))
            .collect()
    };
    let mut head = pad(&rows[0]);
    for (i, h) in head.iter_mut().enumerate() {
        if h.is_empty() {
            *h = format!("列{}", i + 1);
        }
    }
    let mut out = format!("| {} |\n", head.join(" | "));
    out.push_str(&format!("|{}|", vec![" --- "; width].join("|")));
    let body = &rows[1..];
    for r in body.iter().take(MAX_TABLE_ROWS) {
        out.push_str(&format!("\n| {} |", pad(r).join(" | ")));
    }
    if body.len() > MAX_TABLE_ROWS {
        out.push_str(&format!(
            "\n\n(表格较长,仅导入前 {MAX_TABLE_ROWS} 行,共 {} 行)",
            body.len()
        ));
    }
    out
}

fn read_entry_bytes(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    zip.by_name(name)
        .ok()?
        .take(ENTRY_LIMIT)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    read_entry_bytes(zip, name).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn open_zip(path: &Path, what: &str) -> Result<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path)?;
    zip::ZipArchive::new(file).map_err(|e| AppError::Parse(format!("{what} 不是有效的压缩包: {e}")))
}

/// XML 属性值(双引号或单引号)
fn attr(tag: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?:^|[\s<]){}\s*=\s*(?:"([^"]*)"|'([^']*)')"#,
        regex::escape(name)
    ))
    .ok()?;
    let c = re.captures(tag)?;
    c.get(1)
        .or_else(|| c.get(2))
        .map(|m| decode_entities(m.as_str()))
}

/// 相对压缩包内某目录解析路径(处理 `.` 与 `..`,去掉 `?query` / `#fragment` 并解码百分号)
pub fn resolve_zip_path(base_dir: &str, rel: &str) -> String {
    let rel = rel.split(['?', '#']).next().unwrap_or(rel);
    let rel = images::percent_decode(rel);
    let mut parts: Vec<&str> = if rel.starts_with('/') {
        vec![]
    } else {
        base_dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// 取 XML 片段里全部 `<tag ...>文字</tag>` 的文字并拼接
fn texts_of(xml: &str, tag: &str) -> String {
    let re = Regex::new(&format!(r"(?s)<{tag}(?:\s[^>]*)?>([^<]*)</{tag}>")).expect("正则非法");
    re.captures_iter(xml)
        .map(|c| decode_entities(&c[1]))
        .collect::<String>()
}

// ───────────────────────── csv / tsv ─────────────────────────

/// 解析 CSV(RFC 4180:双引号包裹、`""` 转义、字段内可含换行)
pub fn parse_csv(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut field_started = false;
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if !field_started => {
                in_quotes = true;
                field_started = true;
            }
            c if c == sep => {
                row.push(std::mem::take(&mut field));
                field_started = false;
            }
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                field_started = false;
                if row.iter().any(|f| !f.trim().is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            c => {
                field.push(c);
                field_started = true;
            }
        }
    }
    if field_started || !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|f| !f.trim().is_empty()) {
            rows.push(row);
        }
    }
    rows
}

/// 猜测分隔符:取首行里出现最多的 `,` `;` Tab(`.tsv` 固定 Tab)
fn guess_sep(first_line: &str, ext: &str) -> char {
    if ext == "tsv" {
        return '\t';
    }
    [',', ';', '\t']
        .into_iter()
        .max_by_key(|s| first_line.matches(*s).count())
        .filter(|s| first_line.contains(*s))
        .unwrap_or(',')
}

/// csv / tsv → Markdown 表格
pub fn csv_to_markdown(bytes: &[u8], ext: &str) -> String {
    let text = decode_text(bytes);
    let sep = guess_sep(text.lines().next().unwrap_or(""), ext);
    md_table(&parse_csv(&text, sep))
}

// ───────────────────────── xlsx ─────────────────────────

/// 列字母转下标:A→0, Z→25, AA→26
fn col_index(cell_ref: &str) -> usize {
    let mut n = 0usize;
    for c in cell_ref.chars().take_while(|c| c.is_ascii_alphabetic()) {
        n = n * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
    }
    n.saturating_sub(1)
}

/// 内置的日期/时间数字格式 id
fn builtin_date_fmt(id: u32) -> bool {
    matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58)
}

/// 自定义数字格式是否为日期(含 y/m/d/h/s,忽略引号与方括号里的内容)
fn custom_is_date(code: &str) -> bool {
    let mut in_q = false;
    let mut in_b = false;
    let mut found = false;
    for c in code.chars() {
        match c {
            '"' => in_q = !in_q,
            '[' if !in_q => in_b = true,
            ']' if !in_q => in_b = false,
            c if !in_q && !in_b && matches!(c.to_ascii_lowercase(), 'y' | 'd' | 'h' | 's') => {
                found = true
            }
            // m 可能是"月"也可能是"分",单独出现时不能判定
            _ => {}
        }
    }
    found
}

/// Excel 序列日期 → 文本(含时间部分时带 HH:MM)
fn excel_date(serial: f64) -> Option<String> {
    if !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let days = serial.floor() as i64;
    let secs = ((serial - serial.floor()) * 86400.0).round() as i64;
    let base = chrono::NaiveDate::from_ymd_opt(1899, 12, 30)?;
    let d = base.checked_add_signed(chrono::Duration::days(days))?;
    if secs == 0 {
        Some(d.format("%Y-%m-%d").to_string())
    } else {
        let t = chrono::NaiveTime::from_num_seconds_from_midnight_opt((secs % 86400) as u32, 0)?;
        Some(format!("{} {}", d.format("%Y-%m-%d"), t.format("%H:%M")))
    }
}

/// 读取 styles.xml:每个单元格样式索引(cellXfs 顺序)是否为日期格式
fn xlsx_date_styles(styles: &str) -> Vec<bool> {
    let custom_re = Regex::new(r"<numFmt\b[^>]*>").expect("正则非法");
    let mut custom: HashMap<u32, bool> = HashMap::new();
    for m in custom_re.find_iter(styles) {
        if let (Some(id), Some(code)) =
            (attr(m.as_str(), "numFmtId"), attr(m.as_str(), "formatCode"))
        {
            if let Ok(id) = id.parse::<u32>() {
                custom.insert(id, custom_is_date(&code));
            }
        }
    }
    let xfs = Regex::new(r"(?s)<cellXfs\b[^>]*>(.*?)</cellXfs>").expect("正则非法");
    let xf = Regex::new(r"<xf\b[^>]*>").expect("正则非法");
    let Some(block) = xfs.captures(styles) else {
        return vec![];
    };
    xf.find_iter(&block[1])
        .map(|m| {
            attr(m.as_str(), "numFmtId")
                .and_then(|v| v.parse::<u32>().ok())
                .map(|id| {
                    custom
                        .get(&id)
                        .copied()
                        .unwrap_or_else(|| builtin_date_fmt(id))
                })
                .unwrap_or(false)
        })
        .collect()
}

/// 共享字符串表(忽略拼音注音 rPh)
fn xlsx_shared_strings(xml: &str) -> Vec<String> {
    let si = Regex::new(r"(?s)<si\b[^>]*>(.*?)</si>").expect("正则非法");
    let rph = Regex::new(r"(?s)<rPh\b.*?</rPh>").expect("正则非法");
    si.captures_iter(xml)
        .map(|c| texts_of(&rph.replace_all(&c[1], ""), "t"))
        .collect()
}

/// 解析一个工作表为行列数据
fn xlsx_sheet_rows(xml: &str, shared: &[String], date_styles: &[bool]) -> Vec<Vec<String>> {
    let row_re = Regex::new(r"(?s)<row\b[^>]*>(.*?)</row>").expect("正则非法");
    let cell_re = Regex::new(r"(?s)<c\b([^>]*?)(?:/>|>(.*?)</c>)").expect("正则非法");
    let v_re = Regex::new(r"(?s)<v>([^<]*)</v>").expect("正则非法");
    let is_re = Regex::new(r"(?s)<is>(.*?)</is>").expect("正则非法");
    let mut rows: Vec<Vec<String>> = Vec::new();
    for rc in row_re.captures_iter(xml) {
        let mut row: Vec<String> = Vec::new();
        let mut next_col = 0usize;
        for cc in cell_re.captures_iter(&rc[1]) {
            let attrs = &cc[1];
            let inner = cc.get(2).map(|m| m.as_str()).unwrap_or("");
            let col = attr(attrs, "r").map(|r| col_index(&r)).unwrap_or(next_col);
            next_col = col + 1;
            let t = attr(attrs, "t").unwrap_or_default();
            let raw = v_re.captures(inner).map(|c| decode_entities(&c[1]));
            let value = match t.as_str() {
                "s" => raw
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .and_then(|i| shared.get(i).cloned())
                    .unwrap_or_default(),
                "inlineStr" => is_re
                    .captures(inner)
                    .map(|c| texts_of(&c[1], "t"))
                    .unwrap_or_default(),
                "b" => match raw.as_deref() {
                    Some("1") => "是".to_string(),
                    _ => "否".to_string(),
                },
                "str" | "e" => raw.unwrap_or_default(),
                _ => {
                    let style = attr(attrs, "s").and_then(|s| s.parse::<usize>().ok());
                    let is_date = style
                        .and_then(|i| date_styles.get(i).copied())
                        .unwrap_or(false);
                    match raw {
                        Some(v) if is_date => v
                            .trim()
                            .parse::<f64>()
                            .ok()
                            .and_then(excel_date)
                            .unwrap_or(v),
                        Some(v) => v,
                        None => String::new(),
                    }
                }
            };
            if row.len() <= col {
                row.resize(col + 1, String::new());
            }
            row[col] = value;
        }
        if row.iter().any(|c| !c.trim().is_empty()) {
            rows.push(row);
        }
        // 超过上限(多取 2 行:表头与截断提示)后不再继续解析
        if rows.len() > MAX_TABLE_ROWS + 2 {
            break;
        }
    }
    rows
}

/// xlsx → Markdown:每个工作表一个二级标题 + 一张表(隐藏的工作表跳过)
pub fn extract_xlsx(path: &Path) -> Result<String> {
    let mut zip = open_zip(path, "XLSX")?;
    let workbook = read_entry(&mut zip, "xl/workbook.xml")
        .ok_or_else(|| AppError::Parse("XLSX 缺少工作簿信息".into()))?;
    let rels_xml = read_entry(&mut zip, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let mut rels: HashMap<String, String> = HashMap::new();
    for m in Regex::new(r"<Relationship\b[^>]*>")
        .expect("正则非法")
        .find_iter(&rels_xml)
    {
        if let (Some(id), Some(t)) = (attr(m.as_str(), "Id"), attr(m.as_str(), "Target")) {
            rels.insert(id, resolve_zip_path("xl", &t));
        }
    }
    let shared = read_entry(&mut zip, "xl/sharedStrings.xml")
        .map(|x| xlsx_shared_strings(&x))
        .unwrap_or_default();
    let date_styles = read_entry(&mut zip, "xl/styles.xml")
        .map(|x| xlsx_date_styles(&x))
        .unwrap_or_default();

    let mut out: Vec<String> = Vec::new();
    for m in Regex::new(r"<sheet\b[^>]*>")
        .expect("正则非法")
        .find_iter(&workbook)
        .take(MAX_PARTS)
    {
        let tag = m.as_str();
        if matches!(attr(tag, "state").as_deref(), Some("hidden" | "veryHidden")) {
            continue;
        }
        let name = attr(tag, "name").unwrap_or_else(|| "工作表".into());
        let Some(target) = attr(tag, "r:id").and_then(|id| rels.get(&id).cloned()) else {
            continue;
        };
        let Some(xml) = read_entry(&mut zip, &target) else {
            continue;
        };
        let total_rows = xml.matches("<row ").count();
        let rows = xlsx_sheet_rows(&xml, &shared, &date_styles);
        let mut table = md_table(&rows);
        if table.is_empty() {
            continue;
        }
        // 解析阶段提前截断时,提示总行数
        if total_rows > rows.len() && !table.contains("表格较长") {
            table.push_str(&format!(
                "\n\n(表格较长,仅导入前 {} 行,共 {} 行)",
                rows.len().saturating_sub(1),
                total_rows.saturating_sub(1)
            ));
        }
        out.push(format!("## {name}\n\n{table}"));
    }
    Ok(out.join("\n\n"))
}

// ───────────────────────── pptx ─────────────────────────

/// 一页幻灯片的结构
struct Slide {
    xml: String,
    /// rId → 压缩包内完整路径(仅图片关系)
    images: HashMap<String, String>,
    notes: String,
}

fn rels_of(xml: &str, base_dir: &str) -> Vec<(String, String, String)> {
    Regex::new(r"<Relationship\b[^>]*>")
        .expect("正则非法")
        .find_iter(xml)
        .filter_map(|m| {
            let t = m.as_str();
            Some((
                attr(t, "Id")?,
                attr(t, "Type").unwrap_or_default(),
                resolve_zip_path(base_dir, &attr(t, "Target")?),
            ))
        })
        .collect()
}

/// 按演示顺序读取全部幻灯片
fn load_slides(zip: &mut zip::ZipArchive<std::fs::File>) -> Vec<Slide> {
    // 顺序:presentation.xml 里的 sldId 引用顺序;解析失败则按文件名里的数字排序
    let mut order: Vec<String> = Vec::new();
    if let (Some(pres), Some(prels)) = (
        read_entry(zip, "ppt/presentation.xml"),
        read_entry(zip, "ppt/_rels/presentation.xml.rels"),
    ) {
        let map: HashMap<String, String> = rels_of(&prels, "ppt")
            .into_iter()
            .map(|(id, _, t)| (id, t))
            .collect();
        for m in Regex::new(r"<p:sldId\b[^>]*>")
            .expect("正则非法")
            .find_iter(&pres)
        {
            if let Some(t) = attr(m.as_str(), "r:id").and_then(|id| map.get(&id).cloned()) {
                order.push(t);
            }
        }
    }
    if order.is_empty() {
        let num = |s: &str| -> u32 {
            s.trim_start_matches("ppt/slides/slide")
                .trim_end_matches(".xml")
                .parse()
                .unwrap_or(0)
        };
        let mut names: Vec<String> = (0..zip.len())
            .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
            .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
            .collect();
        names.sort_by_key(|n| num(n));
        order = names;
    }
    order
        .into_iter()
        .take(MAX_PARTS)
        .filter_map(|path| {
            let xml = read_entry(zip, &path)?;
            let rels_path = format!("{}/_rels/{}.rels", dir_of(&path), path.rsplit('/').next()?);
            let rels = read_entry(zip, &rels_path).unwrap_or_default();
            let mut images = HashMap::new();
            let mut notes = String::new();
            for (id, ty, target) in rels_of(&rels, dir_of(&path)) {
                if ty.ends_with("/image") {
                    images.insert(id, target);
                } else if ty.ends_with("/notesSlide") {
                    if let Some(n) = read_entry(zip, &target) {
                        notes = slide_notes(&n);
                    }
                }
            }
            Some(Slide { xml, images, notes })
        })
        .collect()
}

/// 备注页文字(不含页码占位)
fn slide_notes(xml: &str) -> String {
    let sp = Regex::new(r"(?s)<p:sp\b.*?</p:sp>").expect("正则非法");
    sp.find_iter(xml)
        .filter(|m| {
            !m.as_str().contains("type=\"sldNum\"") && !m.as_str().contains("type=\"sldImg\"")
        })
        .flat_map(|m| a_paragraphs(m.as_str()))
        .map(|(_, t)| t)
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 一个形状里的段落:(缩进级别, 文字)
fn a_paragraphs(xml: &str) -> Vec<(usize, String)> {
    let p = Regex::new(r"(?s)<a:p\b.*?</a:p>").expect("正则非法");
    let tok = Regex::new(r"(?s)<a:t(?:\s[^>]*)?>([^<]*)</a:t>|<a:br\s*/>").expect("正则非法");
    let lvl = Regex::new(r#"<a:pPr\b[^>]*\blvl="(\d)""#).expect("正则非法");
    p.find_iter(xml)
        .map(|m| {
            let s = m.as_str();
            let text: String = tok
                .captures_iter(s)
                .map(|c| match c.get(1) {
                    Some(t) => decode_entities(t.as_str()),
                    None => " ".to_string(),
                })
                .collect();
            let level = lvl
                .captures(s)
                .and_then(|c| c[1].parse::<usize>().ok())
                .unwrap_or(0);
            (level, text.trim().to_string())
        })
        .collect()
}

/// 一页幻灯片 → (标题, 正文 Markdown)
fn slide_to_markdown(s: &Slide) -> (String, String) {
    let shapes = Regex::new(
        r"(?s)<p:sp\b.*?</p:sp>|<p:graphicFrame\b.*?</p:graphicFrame>|<p:pic\b.*?</p:pic>",
    )
    .expect("正则非法");
    let blip = Regex::new(r#"<a:blip\b[^>]*?\br:embed="([^"]+)""#).expect("正则非法");
    let tr = Regex::new(r"(?s)<a:tr\b.*?</a:tr>").expect("正则非法");
    let tc = Regex::new(r"(?s)<a:tc\b.*?</a:tc>").expect("正则非法");
    let mut title = String::new();
    let mut blocks: Vec<String> = Vec::new();
    for m in shapes.find_iter(&s.xml) {
        let sh = m.as_str();
        if sh.starts_with("<p:sp") {
            if ["sldNum", "dt", "ftr"]
                .iter()
                .any(|t| sh.contains(&format!("type=\"{t}\"")))
            {
                continue;
            }
            let paras = a_paragraphs(sh);
            if sh.contains("type=\"title\"") || sh.contains("type=\"ctrTitle\"") {
                let t = paras
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                if title.is_empty() {
                    title = t;
                } else if !t.is_empty() {
                    blocks.push(t);
                }
                continue;
            }
            let lines: Vec<String> = paras
                .into_iter()
                .filter(|(_, t)| !t.is_empty())
                .map(|(l, t)| format!("{}- {t}", "  ".repeat(l)))
                .collect();
            if !lines.is_empty() {
                blocks.push(lines.join("\n"));
            }
        } else if sh.starts_with("<p:graphicFrame") {
            if !sh.contains("<a:tbl") {
                continue;
            }
            let rows: Vec<Vec<String>> = tr
                .find_iter(sh)
                .map(|r| {
                    tc.find_iter(r.as_str())
                        .map(|c| {
                            a_paragraphs(c.as_str())
                                .into_iter()
                                .map(|(_, t)| t)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .collect()
                })
                .collect();
            let t = md_table(&rows);
            if !t.is_empty() {
                blocks.push(t);
            }
        } else if let Some(target) = blip.captures(sh).and_then(|c| s.images.get(&c[1]).cloned()) {
            let alt = attr(sh, "descr").unwrap_or_default();
            blocks.push(format!("![{alt}]({target})"));
        }
    }
    (title, blocks.join("\n\n"))
}

/// pptx → Markdown:每页一个二级标题(页码 + 标题),正文按层级列表,表格转 Markdown 表格,备注另起一段
pub fn extract_pptx(path: &Path) -> Result<String> {
    let mut zip = open_zip(path, "PPTX")?;
    let slides = load_slides(&mut zip);
    let mut out: Vec<String> = Vec::new();
    for (i, s) in slides.iter().enumerate() {
        let (title, body) = slide_to_markdown(s);
        if title.is_empty() && body.is_empty() && s.notes.trim().is_empty() {
            continue;
        }
        let head = if title.is_empty() {
            format!("## 第{}页", i + 1)
        } else {
            format!("## 第{}页 {title}", i + 1)
        };
        let mut part = head;
        if !body.is_empty() {
            part.push_str(&format!("\n\n{body}"));
        }
        if !s.notes.trim().is_empty() {
            part.push_str(&format!("\n\n备注:{}", s.notes.replace('\n', " ")));
        }
        out.push(part);
    }
    Ok(out.join("\n\n"))
}

/// pptx 内嵌图片(按幻灯片顺序),锚点为压缩包内完整路径
pub fn pptx_images(path: &Path) -> Result<(Vec<EmbeddedImage>, Vec<String>)> {
    let mut zip = open_zip(path, "PPTX")?;
    let slides = load_slides(&mut zip);
    let blip = Regex::new(r#"<a:blip\b[^>]*?\br:embed="([^"]+)""#).expect("正则非法");
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    'outer: for s in &slides {
        for c in blip.captures_iter(&s.xml) {
            let Some(target) = s.images.get(&c[1]) else {
                continue;
            };
            if !seen.insert(target.clone()) {
                continue;
            }
            let Some(bytes) = read_entry_bytes(&mut zip, target) else {
                continue;
            };
            if !images::push_embedded(&mut out, &mut skipped, target, bytes) {
                break 'outer;
            }
        }
    }
    Ok((out, skipped))
}

// ───────────────────────── html ─────────────────────────

/// HTML → 近似 Markdown 的纯文本
///
/// - 去掉 script/style/注释/导航噪音,保留标题(`#`)、列表(`- `)、表格(Markdown 表格)、图片(`![alt](src)`)
/// - base: 压缩包内当前文件所在目录(epub 章节用),给定时图片地址解析为包内完整路径作为锚点
pub fn html_to_text(html: &str, base: Option<&str>) -> String {
    let drop = |pat: &str, s: String| -> String {
        Regex::new(pat)
            .expect("正则非法")
            .replace_all(&s, " ")
            .into_owned()
    };
    let mut s = html.to_string();
    // 只取 <body>(没有则全文)
    if let Some(m) = Regex::new(r"(?i)<body").expect("正则非法").find(&s) {
        s = s[m.start()..].to_string();
    }
    for pat in [
        r"(?is)<!--.*?-->",
        r"(?is)<script\b.*?</script>",
        r"(?is)<style\b.*?</style>",
        r"(?is)<noscript\b.*?</noscript>",
        r"(?is)<svg\b.*?</svg>",
        r"(?is)<nav\b.*?</nav>",
        r"(?is)<head\b.*?</head>",
    ] {
        s = drop(pat, s);
    }
    // 表格:先整体转成 Markdown 表格,避免被后面的标签规则拆散
    let table_re = Regex::new(r"(?is)<table\b.*?</table>").expect("正则非法");
    let tr_re = Regex::new(r"(?is)<tr\b.*?</tr>").expect("正则非法");
    let td_re = Regex::new(r"(?is)<t[dh]\b[^>]*>(.*?)</t[dh]>").expect("正则非法");
    s = table_re
        .replace_all(&s, |c: &regex::Captures| {
            let rows: Vec<Vec<String>> = tr_re
                .find_iter(&c[0])
                .map(|r| {
                    td_re
                        .captures_iter(r.as_str())
                        .map(|d| decode_entities(&strip_tags(&d[1])))
                        .collect()
                })
                .collect();
            format!("\n\n{}\n\n", md_table(&rows))
        })
        .into_owned();
    // 图片 → ![alt](src)
    let img_re = Regex::new(r"(?is)<img\b[^>]*>").expect("正则非法");
    s = img_re
        .replace_all(&s, |c: &regex::Captures| {
            let tag = &c[0];
            let Some(src) = images::html_attr_value(tag, "src") else {
                return String::new();
            };
            let alt = images::html_attr_value(tag, "alt").unwrap_or_default();
            let src = decode_entities(src.trim());
            let src = match base {
                Some(b) if !images::is_remote(&src) => resolve_zip_path(b, &src),
                _ => src,
            };
            format!(
                "\n\n![{}]({src})\n\n",
                decode_entities(&alt).replace(']', "")
            )
        })
        .into_owned();
    // 标题
    let h_re = Regex::new(r"(?is)<h([1-6])\b[^>]*>(.*?)</h[1-6]>").expect("正则非法");
    s = h_re
        .replace_all(&s, |c: &regex::Captures| {
            let level: usize = c[1].parse().unwrap_or(1);
            let t = strip_tags(&c[2]);
            let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
            if t.is_empty() {
                String::new()
            } else {
                format!("\n\n{} {t}\n\n", "#".repeat(level))
            }
        })
        .into_owned();
    // 列表项与换行、块级元素
    s = Regex::new(r"(?is)<li\b[^>]*>")
        .expect("正则非法")
        .replace_all(&s, "\n- ")
        .into_owned();
    s = Regex::new(r"(?is)<br\s*/?>")
        .expect("正则非法")
        .replace_all(&s, "\n")
        .into_owned();
    s = Regex::new(r"(?is)</?(?:p|div|section|article|header|footer|main|aside|ul|ol|blockquote|pre|tr|hr|figure|figcaption|dl|dt|dd)\b[^>]*>")
        .expect("正则非法")
        .replace_all(&s, "\n\n")
        .into_owned();
    let text = decode_entities(&strip_tags(&s));
    // 行内空白折叠(保留换行),每行去首尾空白
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    crate::extract::clean_text(&lines.join("\n"))
}

/// 去掉全部标签
fn strip_tags(s: &str) -> String {
    Regex::new(r"(?s)<[^>]*>")
        .expect("正则非法")
        .replace_all(s, "")
        .into_owned()
}

/// html 文件 → 文本
pub fn extract_html(bytes: &[u8]) -> String {
    // 编码:优先 UTF-8;meta 声明 gbk/gb2312 时按 GBK 解码
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_lowercase();
    let text = if (head.contains("charset=gb") || head.contains("charset=\"gb"))
        && std::str::from_utf8(bytes).is_err()
    {
        encoding_rs::GBK.decode(bytes).0.into_owned()
    } else {
        decode_text(bytes)
    };
    let title = Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
        .expect("正则非法")
        .captures(&text)
        .map(|c| decode_entities(strip_tags(&c[1]).trim()))
        .unwrap_or_default();
    let body = html_to_text(&text, None);
    // 正文里没有一级标题时,用 <title> 补一个,保证切片有标题路径
    if !title.is_empty() && !body.lines().any(|l| l.starts_with("# ")) {
        format!("# {title}\n\n{body}")
    } else {
        body
    }
}

// ───────────────────────── epub ─────────────────────────

/// 读取 epub 的阅读顺序:返回 (章节完整路径列表)
fn epub_spine(zip: &mut zip::ZipArchive<std::fs::File>) -> Result<Vec<String>> {
    let container = read_entry(zip, "META-INF/container.xml")
        .ok_or_else(|| AppError::Parse("EPUB 缺少 container.xml".into()))?;
    let opf_path = Regex::new(r"<rootfile\b[^>]*>")
        .expect("正则非法")
        .find(&container)
        .and_then(|m| attr(m.as_str(), "full-path"))
        .ok_or_else(|| AppError::Parse("EPUB 缺少 OPF 路径".into()))?;
    let opf =
        read_entry(zip, &opf_path).ok_or_else(|| AppError::Parse("EPUB 缺少 OPF 文件".into()))?;
    let base = dir_of(&opf_path).to_string();
    let mut manifest: HashMap<String, String> = HashMap::new();
    for m in Regex::new(r"<item\b[^>]*>")
        .expect("正则非法")
        .find_iter(&opf)
    {
        let t = m.as_str();
        let ty = attr(t, "media-type").unwrap_or_default();
        if ty.contains("html") {
            if let (Some(id), Some(href)) = (attr(t, "id"), attr(t, "href")) {
                manifest.insert(id, resolve_zip_path(&base, &href));
            }
        }
    }
    let spine: Vec<String> = Regex::new(r"<itemref\b[^>]*>")
        .expect("正则非法")
        .find_iter(&opf)
        .filter_map(|m| attr(m.as_str(), "idref").and_then(|id| manifest.get(&id).cloned()))
        .take(MAX_PARTS)
        .collect();
    Ok(spine)
}

/// epub → 文本:按脊柱(spine)顺序拼接各章节
pub fn extract_epub(path: &Path) -> Result<String> {
    let mut zip = open_zip(path, "EPUB")?;
    let mut parts: Vec<String> = Vec::new();
    for chapter in epub_spine(&mut zip)? {
        let Some(bytes) = read_entry_bytes(&mut zip, &chapter) else {
            continue;
        };
        let text = html_to_text(&decode_text(&bytes), Some(dir_of(&chapter)));
        if !text.trim().is_empty() {
            parts.push(text);
        }
    }
    Ok(parts.join("\n\n"))
}

/// epub 内嵌图片(按章节出现顺序),锚点为压缩包内完整路径
pub fn epub_images(path: &Path) -> Result<(Vec<EmbeddedImage>, Vec<String>)> {
    let mut zip = open_zip(path, "EPUB")?;
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    'outer: for chapter in epub_spine(&mut zip)? {
        let Some(bytes) = read_entry_bytes(&mut zip, &chapter) else {
            continue;
        };
        let html = decode_text(&bytes);
        for r in images::scan_refs(&html) {
            if images::is_remote(&r.src) {
                continue;
            }
            let full = resolve_zip_path(dir_of(&chapter), &r.src);
            if !seen.insert(full.clone()) {
                continue;
            }
            let Some(img) = read_entry_bytes(&mut zip, &full) else {
                continue;
            };
            if !images::push_embedded(&mut out, &mut skipped, &full, img) {
                break 'outer;
            }
        }
    }
    Ok((out, skipped))
}

// ───────────────────────── rtf ─────────────────────────

fn rtf_encoding(codepage: u32) -> &'static encoding_rs::Encoding {
    match codepage {
        936 => encoding_rs::GBK,
        950 => encoding_rs::BIG5,
        932 => encoding_rs::SHIFT_JIS,
        949 => encoding_rs::EUC_KR,
        65001 => encoding_rs::UTF_8,
        _ => encoding_rs::WINDOWS_1252,
    }
}

/// RTF → 纯文本
///
/// 处理 `\par` 换行、`\'hh` 按代码页解码(中文常见 GBK)、`\uN` Unicode,
/// 跳过字体表/颜色表/样式表/图片等目的地组。
pub fn rtf_to_text(bytes: &[u8]) -> String {
    let src: Vec<char> = bytes.iter().map(|b| *b as char).collect();
    let mut out = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut codepage = 1252u32;
    // 组栈:(是否跳过, 遇到 \uN 后需要跳过的回退字符数)
    let mut stack: Vec<(bool, usize)> = Vec::new();
    let (mut skip, mut uc) = (false, 1usize);
    let mut skip_chars = 0usize;
    let flush = |pending: &mut Vec<u8>, out: &mut String, cp: u32| {
        if !pending.is_empty() {
            out.push_str(&rtf_encoding(cp).decode(pending).0);
            pending.clear();
        }
    };
    let mut i = 0usize;
    while i < src.len() {
        let c = src[i];
        match c {
            '{' => {
                flush(&mut pending, &mut out, codepage);
                stack.push((skip, uc));
                i += 1;
            }
            '}' => {
                flush(&mut pending, &mut out, codepage);
                if let Some((s, u)) = stack.pop() {
                    skip = s;
                    uc = u;
                }
                i += 1;
            }
            '\\' => {
                i += 1;
                let Some(&n) = src.get(i) else { break };
                if matches!(n, '\\' | '{' | '}') {
                    if !skip {
                        flush(&mut pending, &mut out, codepage);
                        out.push(n);
                    }
                    i += 1;
                } else if n == '\'' {
                    let hex: String = src.iter().skip(i + 1).take(2).collect();
                    if let Ok(b) = u8::from_str_radix(&hex, 16) {
                        if skip_chars > 0 {
                            skip_chars -= 1;
                        } else if !skip {
                            pending.push(b);
                        }
                    }
                    i += 3;
                } else if n == '*' {
                    // 可忽略的目的地(扩展组)
                    skip = true;
                    i += 1;
                } else if n.is_ascii_alphabetic() {
                    let start = i;
                    while i < src.len() && src[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word: String = src[start..i].iter().collect();
                    // 可选的数字参数
                    let num_start = i;
                    if i < src.len() && src[i] == '-' {
                        i += 1;
                    }
                    while i < src.len() && src[i].is_ascii_digit() {
                        i += 1;
                    }
                    let param: Option<i64> =
                        src[num_start..i].iter().collect::<String>().parse().ok();
                    // 控制字后的一个空格是分隔符
                    if i < src.len() && src[i] == ' ' {
                        i += 1;
                    }
                    match word.as_str() {
                        "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "header"
                        | "footer" | "headerl" | "headerr" | "footerl" | "footerr"
                        | "themedata" | "colorschememapping" | "datastore" | "latentstyles"
                        | "listtable" | "listoverridetable" | "rsidtbl" | "generator" => {
                            skip = true
                        }
                        "ansicpg" => codepage = param.unwrap_or(1252) as u32,
                        "uc" => uc = param.unwrap_or(1).max(0) as usize,
                        "bin" => i += param.unwrap_or(0).max(0) as usize,
                        "par" | "line" | "sect" | "page" if !skip => {
                            flush(&mut pending, &mut out, codepage);
                            out.push('\n');
                        }
                        "tab" if !skip => {
                            flush(&mut pending, &mut out, codepage);
                            out.push('\t');
                        }
                        "emdash" if !skip => out.push('—'),
                        "endash" if !skip => out.push('–'),
                        "bullet" if !skip => out.push('•'),
                        "lquote" | "rquote" if !skip => out.push('\''),
                        "ldblquote" | "rdblquote" if !skip => out.push('"'),
                        "u" if !skip => {
                            flush(&mut pending, &mut out, codepage);
                            if let Some(p) = param {
                                let code = if p < 0 { p + 65536 } else { p } as u32;
                                if let Some(ch) = char::from_u32(code) {
                                    out.push(ch);
                                }
                            }
                            skip_chars = uc;
                        }
                        _ => {}
                    }
                } else {
                    // 其它控制符号(如 `\~` 不间断空格、`\-`)
                    if !skip && n == '~' {
                        out.push(' ');
                    }
                    i += 1;
                }
            }
            '\r' | '\n' => i += 1,
            _ => {
                if skip_chars > 0 {
                    skip_chars -= 1;
                } else if !skip {
                    flush(&mut pending, &mut out, codepage);
                    out.push(c);
                }
                i += 1;
            }
        }
    }
    flush(&mut pending, &mut out, codepage);
    crate::extract::clean_text(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn 实体解码() {
        assert_eq!(
            decode_entities("a&amp;b &lt;c&gt; &#65;&#x42; &nbsp;x &unknown;"),
            "a&b <c> AB  x &unknown;"
        );
    }

    #[test]
    fn 字节编码识别() {
        assert_eq!(decode_text("\u{feff}你好".as_bytes()), "你好");
        let (gbk, _, _) = encoding_rs::GBK.encode("预算表");
        assert_eq!(decode_text(&gbk), "预算表");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("你好".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        assert_eq!(decode_text(&utf16), "你好");
    }

    #[test]
    fn csv解析引号与换行() {
        let rows = parse_csv(
            "姓名,备注\n张三,\"说:\"\"好\"\",\n第二行\"\n李四,ok\n\n,\n",
            ',',
        );
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert_eq!(rows[1][1], "说:\"好\",\n第二行");
        assert_eq!(rows[2], vec!["李四", "ok"]);
    }

    #[test]
    fn csv分隔符猜测与gbk() {
        let (gbk, _, _) = encoding_rs::GBK.encode("项目;金额\n差旅;1200\n");
        let md = csv_to_markdown(&gbk, "csv");
        assert!(md.starts_with("| 项目 | 金额 |"), "{md}");
        assert!(md.contains("| 差旅 | 1200 |"));
        assert!(csv_to_markdown(b"a\tb\n1\t2", "tsv").contains("| 1 | 2 |"));
    }

    #[test]
    fn 表格行数超限时截断并提示() {
        let mut rows = vec![vec!["列".to_string()]];
        for i in 0..(MAX_TABLE_ROWS + 10) {
            rows.push(vec![format!("{i}")]);
        }
        let t = md_table(&rows);
        assert!(t.contains(&format!(
            "仅导入前 {MAX_TABLE_ROWS} 行,共 {} 行",
            MAX_TABLE_ROWS + 10
        )));
        // 提示与表格之间有空行,不会破坏表格段落
        assert!(t.contains("|\n\n(表格较长"));
        // 空表头补列名;竖线转义
        assert!(
            md_table(&[vec!["".into(), "b".into()], vec!["x|y".into(), "z".into()]])
                .contains("| 列1 | b |")
        );
        assert!(md_table(&[vec!["a".into()], vec!["x|y".into()]]).contains("x\\|y"));
    }

    #[test]
    fn 列字母与日期() {
        assert_eq!(col_index("A1"), 0);
        assert_eq!(col_index("AB12"), 27);
        assert_eq!(excel_date(45292.0).unwrap(), "2024-01-01");
        assert_eq!(excel_date(45292.5).unwrap(), "2024-01-01 12:00");
        assert!(custom_is_date("yyyy\\-mm\\-dd"));
        // 引号里的字母不算日期格式字符
        assert!(!custom_is_date("0.00\"day\""));
        assert!(!custom_is_date("#,##0.00"));
        assert!(builtin_date_fmt(14) && !builtin_date_fmt(2));
    }

    #[test]
    fn xlsx多工作表日期隐藏表与共享字符串() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("预算.xlsx");
        let wb = r#"<workbook xmlns:r="r"><sheets>
          <sheet name="费用" sheetId="1" r:id="rId1"/>
          <sheet name="隐藏" sheetId="2" state="hidden" r:id="rId2"/>
          <sheet name="汇总" sheetId="3" r:id="rId3"/></sheets></workbook>"#;
        let rels = r#"<Relationships>
          <Relationship Id="rId1" Type="x/worksheet" Target="worksheets/sheet1.xml"/>
          <Relationship Id="rId2" Type="x/worksheet" Target="worksheets/sheet2.xml"/>
          <Relationship Id="rId3" Type="x/worksheet" Target="/xl/worksheets/sheet3.xml"/></Relationships>"#;
        let ss = r#"<sst><si><t>项目</t></si><si><t>金额</t></si><si><r><t>差</t></r><r><t>旅</t></r><rPh><t>ignored</t></rPh></si><si><t>日期</t></si></sst>"#;
        let styles = r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="yyyy\-mm\-dd"/></numFmts>
          <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="164"/></cellXfs></styleSheet>"#;
        let s1 = r#"<worksheet><sheetData>
          <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>3</v></c></row>
          <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>1200</v></c><c r="C2" s="1"><v>45292</v></c></row>
          <row r="3"><c r="A3" t="inlineStr"><is><t>设备</t></is></c><c r="C3" t="b"><v>1</v></c></row>
          <row r="4"><c r="A4"/></row></sheetData></worksheet>"#;
        let s2 = r#"<worksheet><sheetData><row r="1"><c r="A1"><v>9</v></c></row></sheetData></worksheet>"#;
        let s3 = r#"<worksheet><sheetData><row r="1"><c r="A1" t="str"><v>合计</v></c></row></sheetData></worksheet>"#;
        make_zip(
            &p,
            &[
                ("xl/workbook.xml", wb.as_bytes()),
                ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
                ("xl/sharedStrings.xml", ss.as_bytes()),
                ("xl/styles.xml", styles.as_bytes()),
                ("xl/worksheets/sheet1.xml", s1.as_bytes()),
                ("xl/worksheets/sheet2.xml", s2.as_bytes()),
                ("xl/worksheets/sheet3.xml", s3.as_bytes()),
            ],
        );
        let t = extract_xlsx(&p).unwrap();
        assert!(t.contains("## 费用"), "{t}");
        assert!(t.contains("| 项目 | 金额 | 日期 |"), "{t}");
        assert!(t.contains("| 差旅 | 1200 | 2024-01-01 |"), "{t}");
        assert!(t.contains("| 设备 |  | 是 |"), "{t}");
        assert!(!t.contains("隐藏") && !t.contains("ignored"), "{t}");
        assert!(t.contains("## 汇总"), "{t}");
    }

    #[test]
    fn pptx按页提取标题正文表格备注与图片() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("汇报.pptx");
        let pres = r#"<p:presentation xmlns:r="r"><p:sldIdLst><p:sldId id="1" r:id="rId2"/><p:sldId id="2" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
        let prels = r#"<Relationships>
          <Relationship Id="rId1" Type="x/slide" Target="slides/slide1.xml"/>
          <Relationship Id="rId2" Type="x/slide" Target="slides/slide2.xml"/></Relationships>"#;
        // 演示顺序是 slide2 在前
        let s2 = r#"<p:sld><p:cSld><p:spTree>
          <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>季度总结</a:t></a:r></a:p></p:txBody></p:sp>
          <p:sp><p:txBody><a:p><a:pPr lvl="0"/><a:r><a:t>营收增长</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:t>华东 12%</a:t></a:r></a:p></p:txBody></p:sp>
          <p:sp><p:nvSpPr><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>2</a:t></a:r></a:p></p:txBody></p:sp>
          <p:graphicFrame><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>项目</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>金额</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
            <a:tr><a:tc><a:txBody><a:p><a:r><a:t>差旅</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>1200</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></p:graphicFrame>
          <p:pic><p:nvPicPr><p:cNvPr id="4" name="图" descr="趋势图"/></p:nvPicPr><p:blipFill><a:blip r:embed="rId7"/></p:blipFill></p:pic>
          </p:spTree></p:cSld></p:sld>"#;
        let s2rels = r#"<Relationships>
          <Relationship Id="rId7" Type="x/image" Target="../media/image9.png"/>
          <Relationship Id="rId8" Type="x/notesSlide" Target="../notesSlides/notesSlide2.xml"/></Relationships>"#;
        let notes = r#"<p:notes><p:sp><p:txBody><a:p><a:r><a:t>记得强调增长</a:t></a:r></a:p></p:txBody></p:sp></p:notes>"#;
        let s1 = r#"<p:sld><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>封面副标题</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
        let png = vec![7u8; 600];
        let png = png.as_slice();
        make_zip(
            &p,
            &[
                ("ppt/presentation.xml", pres.as_bytes()),
                ("ppt/_rels/presentation.xml.rels", prels.as_bytes()),
                ("ppt/slides/slide1.xml", s1.as_bytes()),
                ("ppt/slides/slide2.xml", s2.as_bytes()),
                ("ppt/slides/_rels/slide2.xml.rels", s2rels.as_bytes()),
                ("ppt/notesSlides/notesSlide2.xml", notes.as_bytes()),
                ("ppt/media/image9.png", png),
            ],
        );
        let t = extract_pptx(&p).unwrap();
        let first = t.find("## 第1页 季度总结").expect(&t);
        let second = t.find("## 第2页").expect(&t);
        assert!(first < second, "按演示顺序:{t}");
        assert!(t.contains("- 营收增长\n  - 华东 12%"), "{t}");
        assert!(
            t.contains("| 项目 | 金额 |") && t.contains("| 差旅 | 1200 |"),
            "{t}"
        );
        assert!(t.contains("![趋势图](ppt/media/image9.png)"), "{t}");
        assert!(t.contains("备注:记得强调增长"), "{t}");
        assert!(!t.contains("\n- 2\n"), "页码占位应被忽略: {t}");
        let (imgs, _) = pptx_images(&p).unwrap();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].anchor, "ppt/media/image9.png");
    }

    #[test]
    fn html转文本保留结构并去掉脚本() {
        let h = r#"<html><head><title>页面标题</title><style>p{}</style></head><body>
          <script>alert(1)</script><!-- 注释 -->
          <h2>第一节</h2><p>正文&amp;内容<br>换行</p>
          <ul><li>条目一</li><li>条目二</li></ul>
          <table><tr><th>项</th><th>值</th></tr><tr><td>a</td><td>1</td></tr></table>
          <img src="img/a.png?x=1&amp;y=2" alt="示意图">
          <img src="https://e.com/x.png"></body></html>"#;
        let t = extract_html(h.as_bytes());
        assert!(t.starts_with("# 页面标题"), "{t}");
        assert!(t.contains("## 第一节"), "{t}");
        assert!(t.contains("正文&内容\n换行"), "{t}");
        assert!(t.contains("- 条目一\n- 条目二"), "{t}");
        assert!(t.contains("| 项 | 值 |") && t.contains("| a | 1 |"), "{t}");
        assert!(t.contains("![示意图](img/a.png?x=1&y=2)"), "{t}");
        assert!(
            !t.contains("alert") && !t.contains("注释") && !t.contains("p{}"),
            "{t}"
        );
    }

    #[test]
    fn 压缩包内路径解析() {
        assert_eq!(
            resolve_zip_path("OEBPS/text", "../images/a%20b.png#x"),
            "OEBPS/images/a b.png"
        );
        assert_eq!(
            resolve_zip_path("ppt/slides", "../media/i.png"),
            "ppt/media/i.png"
        );
        assert_eq!(
            resolve_zip_path("xl", "/xl/worksheets/s.xml"),
            "xl/worksheets/s.xml"
        );
    }

    #[test]
    fn epub按脊柱顺序提取并关联图片() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("书.epub");
        let container = r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let opf = r#"<package><manifest>
          <item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
          <item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
          <item id="img" href="images/a.png" media-type="image/png"/></manifest>
          <spine><itemref idref="c2"/><itemref idref="c1"/></spine></package>"#;
        let ch1 = r#"<html><body><h1>第一章</h1><p>开头</p></body></html>"#;
        let img = vec![9u8; 700];
        let ch2 = r#"<html><body><h1>序章</h1><p>先读这里</p><img src="../images/a.png" alt="插图"/></body></html>"#;
        make_zip(
            &p,
            &[
                ("META-INF/container.xml", container.as_bytes()),
                ("OEBPS/content.opf", opf.as_bytes()),
                ("OEBPS/text/ch1.xhtml", ch1.as_bytes()),
                ("OEBPS/text/ch2.xhtml", ch2.as_bytes()),
                ("OEBPS/images/a.png", img.as_slice()),
            ],
        );
        let t = extract_epub(&p).unwrap();
        assert!(
            t.find("序章").unwrap() < t.find("第一章").unwrap(),
            "脊柱顺序: {t}"
        );
        assert!(t.contains("![插图](OEBPS/images/a.png)"), "{t}");
        let (imgs, _) = epub_images(&p).unwrap();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].anchor, "OEBPS/images/a.png");
    }

    #[test]
    fn rtf提取中英文与跳过字体表() {
        let rtf = br"{\rtf1\ansi\ansicpg936\deff0{\fonttbl{\f0 Arial;}}{\colortbl;\red0\green0\blue0;}\pard Hello\par \'c4\'e3\'ba\'c3\par \u20320?\u22909? world\tab end{\*\generator Word;}\par}";
        let t = rtf_to_text(rtf);
        assert!(t.contains("Hello"), "{t}");
        assert!(t.contains("你好"), "GBK 十六进制转义: {t}");
        assert!(t.contains("你好 world"), "Unicode 转义并跳过回退字符: {t}");
        assert!(!t.contains("Arial") && !t.contains("Word"), "{t}");
        assert!(t.lines().count() >= 3, "{t}");
    }
}
