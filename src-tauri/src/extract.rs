// @author ixxxxoooo
//! 文件类型识别与文本抽取(PDF / Markdown / TXT / DOCX / PPTX / XLSX / CSV / HTML / EPUB / RTF)。
//!
//! 解析在阻塞线程中执行,带 panic 捕获与超时,避免坏文件拖垮应用。

use crate::error::{AppError, Result};
use regex::Regex;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// 文档解析超时
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(180);
/// 允许解析的文本最大字符数(防止超大文件撑爆内存/索引)
pub const MAX_TEXT_CHARS: usize = 2_000_000;

/// 文件分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// 图片
    Image,
    /// 文档
    Document,
    /// 不支持
    Unsupported,
}

/// 根据扩展名分类文件
///
/// 参数: path 文件路径
/// 返回: 文件分类
pub fn classify(path: &Path) -> FileKind {
    match ext_lower(path).as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => FileKind::Image,
        "pdf" | "md" | "markdown" | "txt" | "docx" | "pptx" | "xlsx" | "csv" | "tsv" | "html"
        | "htm" | "xhtml" | "epub" | "rtf" => FileKind::Document,
        _ => FileKind::Unsupported,
    }
}

/// 小写扩展名
pub fn ext_lower(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// 根据扩展名返回 MIME 类型
///
/// 参数: ext 小写扩展名
/// 返回: MIME 字符串
pub fn mime_of(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "html" | "htm" | "xhtml" => "text/html",
        "epub" => "application/epub+zip",
        "rtf" => "application/rtf",
        _ => "application/octet-stream",
    }
}

/// 抽取文档全文
///
/// 参数: path 文档路径
/// 返回: 清洗后的纯文本;空内容/解析失败返回 Parse 错误
pub async fn extract_text(path: &Path) -> Result<String> {
    let p = path.to_path_buf();
    let handle = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extract_text_blocking(&p)))
            .unwrap_or_else(|_| {
                Err(AppError::Parse(
                    "文档解析发生内部错误(文件可能已损坏)".into(),
                ))
            })
    });
    let text = match tokio::time::timeout(EXTRACT_TIMEOUT, handle).await {
        Err(_) => return Err(AppError::Parse("文档解析超时".into())),
        Ok(Err(e)) => return Err(AppError::Parse(format!("解析任务异常: {e}"))),
        Ok(Ok(r)) => r?,
    };
    let text = clean_text(&text);
    if text.trim().is_empty() {
        return Err(AppError::Parse(
            "未能从文档中提取到文字(扫描版 PDF 暂不支持 OCR)".into(),
        ));
    }
    Ok(truncate_chars(text, MAX_TEXT_CHARS))
}

/// 同步抽取文本
fn extract_text_blocking(path: &Path) -> Result<String> {
    match ext_lower(path).as_str() {
        "pdf" => pdf_extract::extract_text(path)
            .map_err(|e| AppError::Parse(format!("PDF 解析失败: {e}"))),
        "docx" => extract_docx(path),
        "pptx" => crate::formats::extract_pptx(path),
        "xlsx" => crate::formats::extract_xlsx(path),
        "csv" | "tsv" => Ok(crate::formats::csv_to_markdown(
            &std::fs::read(path)?,
            &ext_lower(path),
        )),
        "html" | "htm" | "xhtml" => Ok(crate::formats::extract_html(&std::fs::read(path)?)),
        "epub" => crate::formats::extract_epub(path),
        "rtf" => Ok(crate::formats::rtf_to_text(&std::fs::read(path)?)),
        "md" | "markdown" | "txt" => read_text_file(path),
        other => Err(AppError::Invalid(format!("不支持的文件类型: .{other}"))),
    }
}

/// 读取文本文件(自动识别 UTF-8 / UTF-16 / GBK 编码)
fn read_text_file(path: &Path) -> Result<String> {
    Ok(crate::formats::decode_text(&std::fs::read(path)?))
}

/// 截断到指定字符数
fn truncate_chars(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        s
    } else {
        s.chars().take(max).collect()
    }
}

/// 清洗文本:统一换行、去掉行尾空白、压缩连续空行、移除控制字符
///
/// 参数: s 原始文本
/// 返回: 清洗后的文本
pub fn clean_text(s: &str) -> String {
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0;
    for line in s.lines() {
        let line: String = line
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// 解码 XML 实体
fn decode_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 解析 DOCX:读取 word/document.xml,还原段落、标题层级、表格与内嵌图片占位
fn extract_docx(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Parse(format!("DOCX 不是有效的压缩包: {e}")))?;
    let mut xml = String::new();
    {
        let entry = zip
            .by_name("word/document.xml")
            .map_err(|e| AppError::Parse(format!("DOCX 缺少正文: {e}")))?;
        // 限制读取大小,防止压缩炸弹
        entry
            .take(80 * 1024 * 1024)
            .read_to_string(&mut xml)
            .map_err(|e| AppError::Parse(format!("DOCX 正文读取失败: {e}")))?;
    }
    // 图片关系表:正文里的内嵌图片以 `![](media/xxx.png)` 占位,导入时已作为图片记录关联
    let mut rels_xml = String::new();
    if let Ok(entry) = zip.by_name("word/_rels/document.xml.rels") {
        let _ = entry.take(8 * 1024 * 1024).read_to_string(&mut rels_xml);
    }
    // 样式表:中文 Word 的标题样式 id 常是纯数字(如 "1"),需要通过样式名还原层级
    let mut styles_xml = String::new();
    if let Ok(entry) = zip.by_name("word/styles.xml") {
        let _ = entry.take(16 * 1024 * 1024).read_to_string(&mut styles_xml);
    }
    Ok(docx_xml_to_text_full(
        &xml,
        &crate::images::docx_rels(&rels_xml),
        &docx_heading_styles(&styles_xml),
    ))
}

/// 从 styles.xml 读取「样式 id → 标题层级」(样式名为 Heading N / 标题 N / Title)
pub fn docx_heading_styles(styles: &str) -> std::collections::HashMap<String, usize> {
    let style = Regex::new(r"(?s)<w:style\b([^>]*)>(.*?)</w:style>").expect("正则非法");
    let id_re = Regex::new(r#"w:styleId="([^"]*)""#).expect("正则非法");
    let name_re = Regex::new(r#"<w:name w:val="([^"]*)""#).expect("正则非法");
    let mut map = std::collections::HashMap::new();
    for c in style.captures_iter(styles) {
        let (Some(id), Some(name)) = (id_re.captures(&c[1]), name_re.captures(&c[2])) else {
            continue;
        };
        if let Some(level) = heading_level_of_name(&name[1]) {
            map.insert(id[1].to_string(), level);
        }
    }
    map
}

/// 样式名/样式 id 里的标题层级:`Heading 2` / `heading2` / `标题 3` → 2 / 2 / 3;`Title` → 1
fn heading_level_of_name(name: &str) -> Option<usize> {
    let n = name.trim().to_lowercase();
    if n == "title" {
        return Some(1);
    }
    let rest = n
        .strip_prefix("heading")
        .or_else(|| n.strip_prefix("标题"))?;
    rest.trim().parse::<usize>().ok().map(|l| l.clamp(1, 6))
}

/// 把 document.xml 转为文本(可单独测试)
///
/// 参数: xml 正文 XML
/// 返回: 纯文本,标题段落以 Markdown # 前缀表示
pub fn docx_xml_to_text(xml: &str) -> String {
    docx_xml_to_text_full(
        xml,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
    )
}

/// 把 document.xml 转为文本,并把内嵌图片还原为 `![](Target)` 占位(无样式表信息)
pub fn docx_xml_to_text_with(
    xml: &str,
    rels: &std::collections::HashMap<String, String>,
) -> String {
    docx_xml_to_text_full(xml, rels, &std::collections::HashMap::new())
}

/// 把 document.xml 转为文本:段落、标题(样式表/大纲级别)、表格(Markdown 表格,支持合并列)、内嵌图片占位
///
/// 参数: xml 正文 XML;rels 图片关系表(rId → Target);heading_styles 样式 id → 标题层级
pub fn docx_xml_to_text_full(
    xml: &str,
    rels: &std::collections::HashMap<String, String>,
    heading_styles: &std::collections::HashMap<String, usize>,
) -> String {
    let re = Regex::new(
        r#"<w:pStyle w:val="([^"]*)"\s*/>|<w:t(?:\s[^>]*)?>([^<]*)</w:t>|<a:blip\b[^>]*?\br:embed="([^"]+)"|<v:imagedata\b[^>]*?\br:id="([^"]+)"|<w:gridSpan\s+w:val="(\d+)"|<w:outlineLvl\s+w:val="(\d)"|</w:p>|<w:br\s*/>|<w:tab\s*/>|<w:tbl(?:\s[^>]*)?>|</w:tbl>|<w:tr(?:\s[^>]*)?>|</w:tr>|<w:tc(?:\s[^>]*)?>|</w:tc>"#,
    )
    .expect("正则非法");
    let mut out = String::new();
    let mut para = String::new();
    let mut heading: Option<usize> = None;
    // 表格状态:只在最外层表格里建表,嵌套表格的内容并入所在单元格
    let mut depth = 0usize;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut span = 1usize;
    for cap in re.captures_iter(xml) {
        let whole = cap.get(0).unwrap().as_str();
        if let Some(style) = cap.get(1) {
            let id = style.as_str();
            heading = heading_styles
                .get(id)
                .copied()
                .or_else(|| heading_level_of_name(id));
        } else if let Some(t) = cap.get(2) {
            para.push_str(&decode_xml(t.as_str()));
        } else if let Some(rid) = cap.get(3).or_else(|| cap.get(4)) {
            if let Some(target) = rels.get(rid.as_str()) {
                para.push_str(&format!("\n![]({target})\n"));
            }
        } else if let Some(n) = cap.get(5) {
            span = n.as_str().parse().unwrap_or(1).clamp(1, 50);
        } else if let Some(l) = cap.get(6) {
            // 大纲级别 0..8 对应 1..9 级;9 级以上是正文
            if heading.is_none() {
                heading = l
                    .as_str()
                    .parse::<usize>()
                    .ok()
                    .map(|v| v + 1)
                    .filter(|v| *v <= 6);
            }
        } else if whole == "</w:p>" {
            let text = para.trim();
            if depth > 0 {
                if !text.is_empty() {
                    if !cell.is_empty() {
                        cell.push(' ');
                    }
                    cell.push_str(text);
                }
            } else {
                if !text.is_empty() {
                    if let Some(l) = heading {
                        out.push_str(&"#".repeat(l));
                        out.push(' ');
                    }
                    out.push_str(text);
                }
                out.push_str("\n\n");
            }
            para.clear();
            heading = None;
        } else if whole.starts_with("<w:br") {
            para.push('\n');
        } else if whole.starts_with("<w:tab") && whole.ends_with("/>") {
            para.push('\t');
        } else if whole.starts_with("<w:tbl") {
            depth += 1;
            if depth == 1 {
                rows.clear();
            }
        } else if whole == "</w:tbl>" {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                let t = crate::formats::md_table(&rows);
                if !t.is_empty() {
                    out.push_str(&t);
                    out.push_str("\n\n");
                }
                rows.clear();
            }
        } else if whole.starts_with("<w:tr") {
            if depth == 1 {
                row.clear();
            }
        } else if whole == "</w:tr>" {
            if depth == 1 {
                rows.push(std::mem::take(&mut row));
            }
        } else if whole.starts_with("<w:tc") {
            if depth == 1 {
                cell.clear();
                span = 1;
            }
        } else if whole == "</w:tc>" && depth == 1 {
            row.push(std::mem::take(&mut cell));
            // 横向合并的单元格:补空列保持列对齐
            for _ in 1..span {
                row.push(String::new());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 文件分类() {
        assert_eq!(classify(Path::new("a.PNG")), FileKind::Image);
        assert_eq!(classify(Path::new("a.pdf")), FileKind::Document);
        assert_eq!(classify(Path::new("a.exe")), FileKind::Unsupported);
        assert_eq!(classify(Path::new("noext")), FileKind::Unsupported);
    }

    #[test]
    fn 清洗文本压缩空行() {
        let s = clean_text("a  \r\n\r\n\r\n\r\nb\u{0}\n");
        assert_eq!(s, "a\n\nb");
    }

    #[test]
    fn docx解析段落与标题() {
        let xml = r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>标题一</w:t></w:r></w:p>
        <w:p><w:r><w:t xml:space="preserve">正文 &amp; 内容</w:t></w:r></w:p>"#;
        let t = docx_xml_to_text(xml);
        assert!(t.contains("# 标题一"), "{t}");
        assert!(t.contains("正文 & 内容"), "{t}");
    }

    #[test]
    fn docx表格转markdown并支持合并列与嵌套() {
        let xml = r#"<w:body>
          <w:p><w:r><w:t>前言</w:t></w:r></w:p>
          <w:tbl><w:tblPr/><w:tblGrid/>
            <w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>费用表</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>备注</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr><w:tc><w:p><w:r><w:t>差旅</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>1200</w:t></w:r></w:p><w:p><w:r><w:t>含机票</w:t></w:r></w:p></w:tc>
              <w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>嵌套</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/></w:tc></w:tr>
          </w:tbl>
          <w:p><w:r><w:t>后记</w:t></w:r></w:p></w:body>"#;
        let t = docx_xml_to_text(xml);
        assert!(
            t.contains("| 费用表 | 列2 | 备注 |"),
            "合并列补空并给空表头起名: {t}"
        );
        assert!(t.contains("| 差旅 | 1200 含机票 | 嵌套 |"), "{t}");
        // 表格前后的段落仍是独立段落,表格自成一段
        assert!(t.starts_with("前言\n\n| "), "{t}");
        assert!(t.trim_end().ends_with("后记"), "{t}");
    }

    #[test]
    fn docx标题层级来自样式表与大纲级别() {
        let styles = r#"<w:styles>
          <w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/></w:style>
          <w:style w:type="paragraph" w:styleId="a5"><w:name w:val="标题 2"/></w:style>
          <w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/></w:style>
          <w:style w:type="paragraph" w:styleId="a"><w:name w:val="Normal"/></w:style></w:styles>"#;
        let map = docx_heading_styles(styles);
        assert_eq!(map.get("1"), Some(&1));
        assert_eq!(map.get("a5"), Some(&2));
        assert_eq!(map.get("Title"), Some(&1));
        assert!(!map.contains_key("a"));
        let xml = r#"<w:p><w:pPr><w:pStyle w:val="1"/></w:pPr><w:r><w:t>总则</w:t></w:r></w:p>
          <w:p><w:pPr><w:pStyle w:val="a5"/></w:pPr><w:r><w:t>范围</w:t></w:r></w:p>
          <w:p><w:pPr><w:pStyle w:val="a"/><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>细则</w:t></w:r></w:p>
          <w:p><w:pPr><w:pStyle w:val="a"/></w:pPr><w:r><w:t>正文</w:t></w:r></w:p>
          <w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>无样式表也识别</w:t></w:r></w:p>"#;
        let t = docx_xml_to_text_full(xml, &Default::default(), &map);
        assert!(t.contains("# 总则"), "{t}");
        assert!(t.contains("## 范围"), "{t}");
        assert!(t.contains("### 细则"), "{t}");
        assert!(t.contains("\n\n正文"), "{t}");
        assert!(!t.contains("# 正文"), "{t}");
        assert!(t.contains("### 无样式表也识别"), "{t}");
    }

    #[test]
    fn 新格式分类与mime() {
        for e in ["pptx", "xlsx", "csv", "tsv", "html", "htm", "epub", "rtf"] {
            assert_eq!(
                classify(Path::new(&format!("a.{e}"))),
                FileKind::Document,
                "{e}"
            );
            assert_ne!(mime_of(e), "application/octet-stream", "{e}");
        }
        // 旧版二进制 Office 与邮件不支持
        for e in ["doc", "xls", "ppt", "eml"] {
            assert_eq!(
                classify(Path::new(&format!("a.{e}"))),
                FileKind::Unsupported,
                "{e}"
            );
        }
    }

    #[tokio::test]
    async fn 读取文本文件() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        std::fs::write(&p, "\u{feff}# 标题\n\n内容").unwrap();
        let t = extract_text(&p).await.unwrap();
        assert!(t.starts_with("# 标题"));
    }

    #[tokio::test]
    async fn txt按gbk回退解码() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("g.txt");
        let (gbk, _, _) = encoding_rs::GBK.encode("中文内容");
        std::fs::write(&p, gbk).unwrap();
        assert_eq!(extract_text(&p).await.unwrap(), "中文内容");
    }

    #[tokio::test]
    async fn csv与rtf与html走统一入口() {
        let dir = tempfile::tempdir().unwrap();
        let c = dir.path().join("a.csv");
        std::fs::write(&c, "名称,数量\n苹果,3\n").unwrap();
        assert!(extract_text(&c).await.unwrap().contains("| 苹果 | 3 |"));
        let r = dir.path().join("a.rtf");
        std::fs::write(&r, br"{\rtf1\ansi Hello\par World}").unwrap();
        assert_eq!(extract_text(&r).await.unwrap(), "Hello\nWorld");
        let h = dir.path().join("a.html");
        std::fs::write(&h, "<html><body><h1>标题</h1><p>正文</p></body></html>").unwrap();
        assert_eq!(extract_text(&h).await.unwrap(), "# 标题\n\n正文");
    }

    #[tokio::test]
    async fn 空文件报错() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "   \n").unwrap();
        assert!(extract_text(&p).await.is_err());
    }

    #[tokio::test]
    async fn 损坏的pdf不会panic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.pdf");
        std::fs::write(&p, "这不是 PDF").unwrap();
        assert!(extract_text(&p).await.is_err());
    }
}
