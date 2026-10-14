// @author ygw
//! 文件类型识别与文本抽取(PDF / Markdown / TXT / DOCX)。
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
        "pdf" | "md" | "markdown" | "txt" | "docx" => FileKind::Document,
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
        "md" | "markdown" | "txt" => read_text_file(path),
        other => Err(AppError::Invalid(format!("不支持的文件类型: .{other}"))),
    }
}

/// 读取文本文件(容错非 UTF-8 编码)
fn read_text_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    Ok(String::from_utf8_lossy(bytes).into_owned())
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

/// 解析 DOCX:读取 word/document.xml 并还原段落与标题
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
    Ok(docx_xml_to_text(&xml))
}

/// 把 document.xml 转为文本(可单独测试)
///
/// 参数: xml 正文 XML
/// 返回: 纯文本,标题段落以 Markdown # 前缀表示
pub fn docx_xml_to_text(xml: &str) -> String {
    let re = Regex::new(
        r#"<w:pStyle w:val="(?:Heading|heading|标题)\s*(\d)"\s*/>|<w:t(?:\s[^>]*)?>([^<]*)</w:t>|</w:p>|<w:br\s*/>|<w:tab\s*/>"#,
    )
    .expect("正则非法");
    let mut out = String::new();
    let mut para = String::new();
    let mut heading: Option<usize> = None;
    for cap in re.captures_iter(xml) {
        let whole = cap.get(0).unwrap().as_str();
        if let Some(level) = cap.get(1) {
            heading = level.as_str().parse::<usize>().ok().map(|l| l.clamp(1, 6));
        } else if let Some(t) = cap.get(2) {
            para.push_str(&decode_xml(t.as_str()));
        } else if whole == "</w:p>" {
            let text = para.trim();
            if !text.is_empty() {
                if let Some(l) = heading {
                    out.push_str(&"#".repeat(l));
                    out.push(' ');
                }
                out.push_str(text);
            }
            out.push_str("\n\n");
            para.clear();
            heading = None;
        } else if whole.starts_with("<w:br") {
            para.push('\n');
        } else if whole.starts_with("<w:tab") {
            para.push('\t');
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

    #[tokio::test]
    async fn 读取文本文件() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        std::fs::write(&p, "\u{feff}# 标题\n\n内容").unwrap();
        let t = extract_text(&p).await.unwrap();
        assert!(t.starts_with("# 标题"));
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
